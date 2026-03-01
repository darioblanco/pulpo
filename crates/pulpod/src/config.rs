use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use pulpo_common::auth::BindMode;
use pulpo_common::guard::{EnvFilter, GuardConfig, GuardPreset};
use pulpo_common::peer::PeerEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub node: NodeConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub peers: HashMap<String, PeerEntry>,
    #[serde(default)]
    pub guards: GuardDefaultConfig,
    #[serde(default)]
    pub watchdog: WatchdogConfig,
    #[serde(default)]
    pub personas: HashMap<String, PersonaConfig>,
    #[serde(default)]
    pub notifications: NotificationsConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersonaConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub guard_preset: Option<String>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub max_budget_usd: Option<f64>,
    #[serde(default)]
    pub output_format: Option<String>,
}

/// Notification configuration (webhooks for status updates).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NotificationsConfig {
    /// Discord webhook notifications.
    #[serde(default)]
    pub discord: Option<DiscordWebhookConfig>,
}

/// Discord webhook configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscordWebhookConfig {
    /// Discord webhook URL.
    pub webhook_url: String,
    /// Optional event filter — only send notifications for these statuses.
    /// If empty/absent, all events are sent.
    #[serde(default)]
    pub events: Vec<String>,
}

/// Authentication configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AuthConfig {
    /// Bearer token for API authentication (auto-generated on first run).
    #[serde(default)]
    pub token: String,
    /// How the daemon binds to the network.
    #[serde(default)]
    pub bind: BindMode,
}

/// Generate a cryptographically random 256-bit token as a base64url string (44 chars).
pub fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// If the token is empty, generate one. Returns `true` if a new token was generated.
pub fn ensure_auth_token(config: &mut Config) -> bool {
    if config.auth.token.is_empty() {
        config.auth.token = generate_token();
        true
    } else {
        false
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GuardDefaultConfig {
    #[serde(default = "default_preset")]
    pub preset: GuardPreset,
    #[serde(default)]
    pub env: Option<EnvFilter>,
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default)]
    pub max_budget_usd: Option<f64>,
    #[serde(default)]
    pub output_format: Option<String>,
}

impl Default for GuardDefaultConfig {
    fn default() -> Self {
        Self {
            preset: default_preset(),
            env: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        }
    }
}

impl GuardDefaultConfig {
    pub fn to_guard_config(&self) -> GuardConfig {
        let mut config = GuardConfig::from_preset(self.preset);
        if let Some(env) = &self.env {
            config.env = env.clone();
        }
        config
    }
}

const fn default_preset() -> GuardPreset {
    GuardPreset::Standard
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WatchdogConfig {
    #[serde(default = "default_watchdog_enabled")]
    pub enabled: bool,
    #[serde(default = "default_memory_threshold")]
    pub memory_threshold: u8,
    #[serde(default = "default_check_interval_secs")]
    pub check_interval_secs: u64,
    #[serde(default = "default_breach_count")]
    pub breach_count: u32,
    #[serde(default)]
    pub auto_recover: bool,
    #[serde(default = "default_max_recoveries")]
    pub max_recoveries: u32,
    #[serde(default = "default_recovery_backoff_secs")]
    pub recovery_backoff_secs: u64,
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_idle_action")]
    pub idle_action: String,
}

impl WatchdogConfig {
    pub fn validate(&self) -> Result<()> {
        if self.memory_threshold == 0 || self.memory_threshold > 100 {
            anyhow::bail!(
                "watchdog.memory_threshold must be 1-100, got {}",
                self.memory_threshold
            );
        }
        if self.check_interval_secs == 0 {
            anyhow::bail!("watchdog.check_interval_secs must be >= 1");
        }
        if self.breach_count == 0 {
            anyhow::bail!("watchdog.breach_count must be >= 1");
        }
        if self.auto_recover && self.max_recoveries == 0 {
            anyhow::bail!("watchdog.max_recoveries must be >= 1 when auto_recover is enabled");
        }
        if self.auto_recover && self.recovery_backoff_secs == 0 {
            anyhow::bail!(
                "watchdog.recovery_backoff_secs must be >= 1 when auto_recover is enabled"
            );
        }
        if self.idle_action != "alert" && self.idle_action != "kill" {
            anyhow::bail!(
                "watchdog.idle_action must be \"alert\" or \"kill\", got \"{}\"",
                self.idle_action
            );
        }
        Ok(())
    }
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            enabled: default_watchdog_enabled(),
            memory_threshold: default_memory_threshold(),
            check_interval_secs: default_check_interval_secs(),
            breach_count: default_breach_count(),
            auto_recover: false,
            max_recoveries: default_max_recoveries(),
            recovery_backoff_secs: default_recovery_backoff_secs(),
            idle_timeout_secs: default_idle_timeout_secs(),
            idle_action: default_idle_action(),
        }
    }
}

const fn default_watchdog_enabled() -> bool {
    true
}

const fn default_memory_threshold() -> u8 {
    90
}

const fn default_check_interval_secs() -> u64 {
    10
}

const fn default_breach_count() -> u32 {
    3
}

const fn default_max_recoveries() -> u32 {
    3
}

const fn default_recovery_backoff_secs() -> u64 {
    30
}

const fn default_idle_timeout_secs() -> u64 {
    600
}

fn default_idle_action() -> String {
    String::from("alert")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeConfig {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

fn default_name() -> String {
    let fallback = String::from("unknown");
    hostname::get().map_or(fallback, |h| h.to_string_lossy().into_owned())
}

const fn default_port() -> u16 {
    7433
}

fn default_data_dir() -> String {
    let fallback = String::from("~/.pulpo");
    dirs::home_dir().map_or(fallback, |h| {
        h.join(".pulpo").to_string_lossy().into_owned()
    })
}

impl Config {
    pub fn data_dir(&self) -> String {
        shellexpand::tilde(&self.node.data_dir).into_owned()
    }
}

pub fn save(config: &Config, path: &Path) -> Result<()> {
    let content = toml::to_string_pretty(config).context("Failed to serialize config")?;
    let parent = path
        .parent()
        .with_context(|| format!("No parent directory for {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write config to {}", path.display()))?;
    Ok(())
}

pub fn load(path: &str) -> Result<Config> {
    let expanded = shellexpand::tilde(path);
    let path = std::path::Path::new(expanded.as_ref());

    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Config = toml::from_str(&content).context("Failed to parse config")?;
        config.watchdog.validate()?;
        Ok(config)
    } else {
        // Return defaults if no config file exists
        Ok(Config {
            node: NodeConfig {
                name: default_name(),
                port: default_port(),
                data_dir: default_data_dir(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_default_port() {
        assert_eq!(default_port(), 7433);
    }

    #[test]
    fn test_default_name_returns_hostname() {
        let name = default_name();
        assert!(!name.is_empty());
    }

    #[test]
    fn test_default_data_dir_contains_pulpo() {
        let dir = default_data_dir();
        assert!(
            dir.contains(".pulpo"),
            "Expected .pulpo in path, got: {dir}"
        );
    }

    #[test]
    fn test_load_missing_config_returns_defaults() {
        let config = load("/nonexistent/path/config.toml").unwrap();
        assert_eq!(config.node.port, 7433);
        assert!(!config.node.name.is_empty());
    }

    #[test]
    fn test_load_valid_config() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test-node"
port = 9999
data_dir = "/tmp/pulpo-test"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.name, "test-node");
        assert_eq!(config.node.port, 9999);
        assert_eq!(config.node.data_dir, "/tmp/pulpo-test");
    }

    #[test]
    fn test_load_invalid_config() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(tmpfile, "this is not valid toml {{{{").unwrap();

        let result = load(tmpfile.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_data_dir_expansion() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "~/test-pulpo".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        let expanded = config.data_dir();
        assert!(
            !expanded.starts_with('~'),
            "Tilde should be expanded: {expanded}"
        );
        assert!(expanded.ends_with("test-pulpo"));
    }

    #[test]
    fn test_data_dir_no_tilde() {
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/absolute/path".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        assert_eq!(config.data_dir(), "/absolute/path");
    }

    #[test]
    fn test_load_partial_config_uses_defaults() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "partial"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.name, "partial");
        assert_eq!(config.node.port, 7433); // default
    }

    #[cfg(unix)]
    #[test]
    fn test_load_unreadable_config() {
        use std::os::unix::fs::PermissionsExt;

        let tmpfile = tempfile::NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap().to_owned();
        // Remove read permissions
        std::fs::set_permissions(tmpfile.path(), std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = load(&path);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("Failed to read config"),
            "Expected 'Failed to read config' in error: {err_msg}"
        );

        // Restore permissions for cleanup
        std::fs::set_permissions(tmpfile.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn test_load_config_with_peers() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test-node"
port = 7433
data_dir = "/tmp/pulpo-test"

[peers]
win-pc = "192.168.1.100:7433"
macbook = "10.0.0.5:7433"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.peers.len(), 2);
        assert_eq!(
            config.peers["win-pc"],
            PeerEntry::Simple("192.168.1.100:7433".into())
        );
        assert_eq!(
            config.peers["macbook"],
            PeerEntry::Simple("10.0.0.5:7433".into())
        );
    }

    #[test]
    fn test_load_config_without_peers_backward_compat() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "old-config"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.name, "old-config");
        assert!(config.peers.is_empty());
    }

    #[test]
    fn test_load_config_with_empty_peers() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test"

[peers]
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(config.peers.is_empty());
    }

    #[test]
    fn test_missing_config_has_empty_peers() {
        let config = load("/nonexistent/peers/config.toml").unwrap();
        assert!(config.peers.is_empty());
    }

    #[test]
    fn test_config_debug_includes_peers() {
        let mut peers = HashMap::new();
        peers.insert("node-a".into(), PeerEntry::Simple("host:7433".into()));
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers,
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("node-a"));
    }

    #[test]
    fn test_load_config_without_guards_backward_compat() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "no-guards"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(
            config.guards.preset,
            pulpo_common::guard::GuardPreset::Standard
        );
        assert!(config.guards.env.is_none());
    }

    #[test]
    fn test_load_config_with_guards_preset() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "guarded"
port = 7433

[guards]
preset = "strict"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(
            config.guards.preset,
            pulpo_common::guard::GuardPreset::Strict
        );
    }

    #[test]
    fn test_load_config_with_guards_env() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "env-guarded"

[guards]
preset = "strict"

[guards.env]
allow = ["ANTHROPIC_API_KEY", "PATH", "HOME", "TERM"]
deny = ["AWS_*", "SSH_*", "GITHUB_TOKEN"]
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        let env = config.guards.env.as_ref().unwrap();
        assert_eq!(env.allow.len(), 4);
        assert_eq!(env.deny.len(), 3);
        assert!(env.allow.contains(&"ANTHROPIC_API_KEY".into()));
        assert!(env.deny.contains(&"AWS_*".into()));
    }

    #[test]
    fn test_load_config_with_guards_guardrails() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "guardrailed"

[guards]
preset = "standard"
max_turns = 50
max_budget_usd = 10.0
output_format = "stream-json"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.guards.max_turns, Some(50));
        assert_eq!(config.guards.max_budget_usd, Some(10.0));
        assert_eq!(config.guards.output_format, Some("stream-json".into()));
    }

    #[test]
    fn test_guard_default_config_to_guard_config() {
        let gdc = GuardDefaultConfig {
            preset: pulpo_common::guard::GuardPreset::Strict,
            env: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let gc = gdc.to_guard_config();
        assert_eq!(gc.file_write, pulpo_common::guard::FileScope::RepoOnly);
        assert_eq!(gc.shell, pulpo_common::guard::ShellAccess::None);
    }

    #[test]
    fn test_guard_default_config_to_guard_config_with_env() {
        let gdc = GuardDefaultConfig {
            preset: pulpo_common::guard::GuardPreset::Standard,
            env: Some(pulpo_common::guard::EnvFilter {
                allow: vec!["PATH".into()],
                deny: vec!["SECRET".into()],
            }),
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let gc = gdc.to_guard_config();
        assert_eq!(gc.env.allow, vec!["PATH"]);
        assert_eq!(gc.env.deny, vec!["SECRET"]);
    }

    #[test]
    fn test_guard_default_config_default() {
        let gdc = GuardDefaultConfig::default();
        assert_eq!(gdc.preset, pulpo_common::guard::GuardPreset::Standard);
        assert!(gdc.env.is_none());
        assert!(gdc.max_turns.is_none());
        assert!(gdc.max_budget_usd.is_none());
        assert!(gdc.output_format.is_none());
    }

    #[test]
    fn test_guard_default_config_debug() {
        let gdc = GuardDefaultConfig::default();
        let debug = format!("{gdc:?}");
        assert!(debug.contains("Standard"));
    }

    #[test]
    fn test_missing_config_has_default_guards() {
        let config = load("/nonexistent/guards/config.toml").unwrap();
        assert_eq!(
            config.guards.preset,
            pulpo_common::guard::GuardPreset::Standard
        );
    }

    #[test]
    fn test_save_creates_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        let config = Config {
            node: NodeConfig {
                name: "saved".into(),
                port: 8080,
                data_dir: "/tmp/data".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("saved"));
        assert!(content.contains("8080"));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("roundtrip.toml");
        let mut peers = HashMap::new();
        peers.insert("remote".into(), PeerEntry::Simple("10.0.0.1:7433".into()));
        let config = Config {
            node: NodeConfig {
                name: "roundtrip".into(),
                port: 9000,
                data_dir: "/tmp/rt".into(),
            },
            auth: AuthConfig::default(),
            peers,
            guards: GuardDefaultConfig {
                preset: pulpo_common::guard::GuardPreset::Strict,
                env: Some(pulpo_common::guard::EnvFilter {
                    allow: vec!["PATH".into()],
                    deny: vec!["SECRET".into()],
                }),
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
            },
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.name, "roundtrip");
        assert_eq!(loaded.node.port, 9000);
        assert_eq!(
            loaded.peers["remote"],
            PeerEntry::Simple("10.0.0.1:7433".into())
        );
        assert_eq!(
            loaded.guards.preset,
            pulpo_common::guard::GuardPreset::Strict
        );
        let env = loaded.guards.env.unwrap();
        assert_eq!(env.allow, vec!["PATH"]);
        assert_eq!(env.deny, vec!["SECRET"]);
    }

    #[test]
    fn test_save_creates_parent_directories() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("nested/deep/config.toml");
        let config = Config {
            node: NodeConfig {
                name: "nested".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_save_unwritable_path() {
        let result = save(
            &Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: "/tmp".into(),
                },
                auth: AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: NotificationsConfig::default(),
            },
            Path::new("/dev/null/impossible/config.toml"),
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to create config directory"));
    }

    #[test]
    fn test_save_empty_path_no_parent() {
        let result = save(
            &Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: "/tmp".into(),
                },
                auth: AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: NotificationsConfig::default(),
            },
            Path::new(""),
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No parent directory"));
    }

    #[test]
    fn test_save_write_fails_to_directory() {
        // Parent exists and create_dir_all succeeds, but writing to a directory fails
        let tmpdir = tempfile::tempdir().unwrap();
        let dir_target = tmpdir.path().join("is_a_dir");
        std::fs::create_dir_all(&dir_target).unwrap();
        let result = save(
            &Config {
                node: NodeConfig {
                    name: "test".into(),
                    port: 7433,
                    data_dir: "/tmp".into(),
                },
                auth: AuthConfig::default(),
                peers: HashMap::new(),
                guards: GuardDefaultConfig::default(),
                watchdog: WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: NotificationsConfig::default(),
            },
            &dir_target,
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to write config to"));
    }

    #[test]
    fn test_config_clone() {
        let config = Config {
            node: NodeConfig {
                name: "clone-test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = config.clone();
        assert_eq!(cloned.node.name, "clone-test");
    }

    #[test]
    fn test_node_config_clone() {
        let nc = NodeConfig {
            name: "test".into(),
            port: 7433,
            data_dir: "/tmp".into(),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = nc.clone();
        assert_eq!(cloned.name, "test");
    }

    #[test]
    fn test_guard_default_config_clone() {
        let gdc = GuardDefaultConfig::default();
        #[allow(clippy::redundant_clone)]
        let cloned = gdc.clone();
        assert_eq!(cloned.preset, pulpo_common::guard::GuardPreset::Standard);
    }

    #[test]
    fn test_config_serialize() {
        let config = Config {
            node: NodeConfig {
                name: "ser".into(),
                port: 1234,
                data_dir: "/d".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("ser"));
        assert!(toml_str.contains("1234"));
    }

    #[test]
    fn test_auth_config_default() {
        let auth = AuthConfig::default();
        assert!(auth.token.is_empty());
        assert_eq!(auth.bind, pulpo_common::auth::BindMode::Local);
    }

    #[test]
    fn test_auth_config_debug() {
        let auth = AuthConfig::default();
        let debug = format!("{auth:?}");
        assert!(debug.contains("AuthConfig"));
    }

    #[test]
    fn test_auth_config_clone() {
        let auth = AuthConfig {
            token: "test-token".into(),
            bind: pulpo_common::auth::BindMode::Lan,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = auth.clone();
        assert_eq!(cloned.token, "test-token");
        assert_eq!(cloned.bind, pulpo_common::auth::BindMode::Lan);
    }

    #[test]
    fn test_generate_token_length() {
        let token = generate_token();
        // 32 bytes → 43-44 chars in base64url (no padding → 43 chars)
        assert_eq!(token.len(), 43);
    }

    #[test]
    fn test_generate_token_uniqueness() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_ne!(t1, t2, "Two generated tokens should differ");
    }

    #[test]
    fn test_generate_token_is_base64url() {
        let token = generate_token();
        // base64url chars: A-Z, a-z, 0-9, -, _
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "Token should be base64url: {token}"
        );
    }

    #[test]
    fn test_ensure_auth_token_generates_when_empty() {
        let mut config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        assert!(config.auth.token.is_empty());
        let generated = ensure_auth_token(&mut config);
        assert!(generated);
        assert!(!config.auth.token.is_empty());
        assert_eq!(config.auth.token.len(), 43);
    }

    #[test]
    fn test_ensure_auth_token_preserves_existing() {
        let mut config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig {
                token: "existing-token".into(),
                bind: pulpo_common::auth::BindMode::Lan,
            },
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        let generated = ensure_auth_token(&mut config);
        assert!(!generated);
        assert_eq!(config.auth.token, "existing-token");
    }

    #[test]
    fn test_load_config_without_auth_backward_compat() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "old-config"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.name, "old-config");
        // Auth should default
        assert!(config.auth.token.is_empty());
        assert_eq!(config.auth.bind, pulpo_common::auth::BindMode::Local);
    }

    #[test]
    fn test_load_config_with_auth() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "authed"
port = 7433

[auth]
token = "my-secret-token"
bind = "lan"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.auth.token, "my-secret-token");
        assert_eq!(config.auth.bind, pulpo_common::auth::BindMode::Lan);
    }

    #[test]
    fn test_save_and_load_roundtrip_with_auth() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("auth-roundtrip.toml");
        let config = Config {
            node: NodeConfig {
                name: "auth-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig {
                token: "roundtrip-token".into(),
                bind: pulpo_common::auth::BindMode::Lan,
            },
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.auth.token, "roundtrip-token");
        assert_eq!(loaded.auth.bind, pulpo_common::auth::BindMode::Lan);
    }

    #[test]
    fn test_missing_config_has_default_auth() {
        let config = load("/nonexistent/auth/config.toml").unwrap();
        assert!(config.auth.token.is_empty());
        assert_eq!(config.auth.bind, pulpo_common::auth::BindMode::Local);
    }

    #[test]
    fn test_load_config_with_peer_tokens() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test"
port = 7433

[peers]
mac = "mac:7433"

[peers.win]
address = "win:7433"
token = "peer-secret"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.peers.len(), 2);
        assert_eq!(config.peers["mac"].address(), "mac:7433");
        assert_eq!(config.peers["mac"].token(), None);
        assert_eq!(config.peers["win"].address(), "win:7433");
        assert_eq!(config.peers["win"].token(), Some("peer-secret"));
    }

    #[test]
    fn test_save_and_load_roundtrip_with_peer_tokens() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("peer-tok.toml");
        let mut peers = HashMap::new();
        peers.insert("simple".into(), PeerEntry::Simple("s:1".into()));
        peers.insert(
            "full".into(),
            PeerEntry::Full {
                address: "f:1".into(),
                token: Some("tok".into()),
            },
        );
        let config = Config {
            node: NodeConfig {
                name: "rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers,
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.peers["simple"].address(), "s:1");
        assert_eq!(loaded.peers["simple"].token(), None);
        assert_eq!(loaded.peers["full"].address(), "f:1");
        assert_eq!(loaded.peers["full"].token(), Some("tok"));
    }

    #[test]
    fn test_watchdog_config_default() {
        let wc = WatchdogConfig::default();
        assert!(wc.enabled);
        assert_eq!(wc.memory_threshold, 90);
        assert_eq!(wc.check_interval_secs, 10);
        assert_eq!(wc.breach_count, 3);
        assert!(!wc.auto_recover);
        assert_eq!(wc.max_recoveries, 3);
        assert_eq!(wc.recovery_backoff_secs, 30);
        assert_eq!(wc.idle_timeout_secs, 600);
        assert_eq!(wc.idle_action, "alert");
    }

    #[test]
    fn test_watchdog_config_debug() {
        let wc = WatchdogConfig::default();
        let debug = format!("{wc:?}");
        assert!(debug.contains("enabled"));
        assert!(debug.contains("90"));
    }

    #[test]
    fn test_watchdog_config_clone() {
        let wc = WatchdogConfig::default();
        #[allow(clippy::redundant_clone)]
        let cloned = wc.clone();
        assert!(cloned.enabled);
        assert_eq!(cloned.memory_threshold, 90);
    }

    #[test]
    fn test_load_config_without_watchdog_backward_compat() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "no-watchdog"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.name, "no-watchdog");
        assert!(config.watchdog.enabled);
        assert_eq!(config.watchdog.memory_threshold, 90);
        assert_eq!(config.watchdog.check_interval_secs, 10);
        assert_eq!(config.watchdog.breach_count, 3);
    }

    #[test]
    fn test_load_config_with_watchdog_custom_values() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "custom-wd"

[watchdog]
enabled = false
memory_threshold = 80
check_interval_secs = 5
breach_count = 5
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(!config.watchdog.enabled);
        assert_eq!(config.watchdog.memory_threshold, 80);
        assert_eq!(config.watchdog.check_interval_secs, 5);
        assert_eq!(config.watchdog.breach_count, 5);
    }

    #[test]
    fn test_save_and_load_roundtrip_with_watchdog() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("wd-roundtrip.toml");
        let config = Config {
            node: NodeConfig {
                name: "wd-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig {
                enabled: false,
                memory_threshold: 75,
                check_interval_secs: 30,
                breach_count: 5,
                auto_recover: false,
                max_recoveries: 3,
                recovery_backoff_secs: 30,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
            },
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert!(!loaded.watchdog.enabled);
        assert_eq!(loaded.watchdog.memory_threshold, 75);
        assert_eq!(loaded.watchdog.check_interval_secs, 30);
        assert_eq!(loaded.watchdog.breach_count, 5);
    }

    #[test]
    fn test_missing_config_has_default_watchdog() {
        let config = load("/nonexistent/watchdog/config.toml").unwrap();
        assert!(config.watchdog.enabled);
        assert_eq!(config.watchdog.memory_threshold, 90);
    }

    #[test]
    fn test_load_config_with_partial_watchdog() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "partial-wd"

[watchdog]
enabled = false
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(!config.watchdog.enabled);
        assert_eq!(config.watchdog.memory_threshold, 90); // default
        assert_eq!(config.watchdog.check_interval_secs, 10); // default
        assert_eq!(config.watchdog.breach_count, 3); // default
    }

    #[test]
    fn test_watchdog_validate_defaults_pass() {
        let wd = WatchdogConfig::default();
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_watchdog_validate_threshold_zero() {
        let wd = WatchdogConfig {
            memory_threshold: 0,
            ..WatchdogConfig::default()
        };
        let err = wd.validate().unwrap_err();
        assert!(err.to_string().contains("memory_threshold"));
    }

    #[test]
    fn test_watchdog_validate_interval_zero() {
        let wd = WatchdogConfig {
            check_interval_secs: 0,
            ..WatchdogConfig::default()
        };
        let err = wd.validate().unwrap_err();
        assert!(err.to_string().contains("check_interval_secs"));
    }

    #[test]
    fn test_watchdog_validate_breach_count_zero() {
        let wd = WatchdogConfig {
            breach_count: 0,
            ..WatchdogConfig::default()
        };
        let err = wd.validate().unwrap_err();
        assert!(err.to_string().contains("breach_count"));
    }

    #[test]
    fn test_watchdog_validate_threshold_boundary() {
        // threshold=1 should pass
        let wd = WatchdogConfig {
            memory_threshold: 1,
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());

        // threshold=100 should pass
        let wd = WatchdogConfig {
            memory_threshold: 100,
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_load_config_rejects_invalid_watchdog() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "bad-wd"

[watchdog]
memory_threshold = 0
"#
        )
        .unwrap();

        let result = load(tmpfile.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("memory_threshold"));
    }

    #[test]
    fn test_watchdog_validate_auto_recover_max_zero() {
        let wd = WatchdogConfig {
            auto_recover: true,
            max_recoveries: 0,
            ..WatchdogConfig::default()
        };
        let err = wd.validate().unwrap_err();
        assert!(err.to_string().contains("max_recoveries"));
    }

    #[test]
    fn test_watchdog_validate_auto_recover_backoff_zero() {
        let wd = WatchdogConfig {
            auto_recover: true,
            recovery_backoff_secs: 0,
            ..WatchdogConfig::default()
        };
        let err = wd.validate().unwrap_err();
        assert!(err.to_string().contains("recovery_backoff_secs"));
    }

    #[test]
    fn test_watchdog_validate_auto_recover_disabled_ignores_zeros() {
        let wd = WatchdogConfig {
            auto_recover: false,
            max_recoveries: 0,
            recovery_backoff_secs: 0,
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_watchdog_validate_auto_recover_valid() {
        let wd = WatchdogConfig {
            auto_recover: true,
            max_recoveries: 5,
            recovery_backoff_secs: 60,
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_load_config_with_auto_recover() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "recover"

[watchdog]
auto_recover = true
max_recoveries = 5
recovery_backoff_secs = 60
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(config.watchdog.auto_recover);
        assert_eq!(config.watchdog.max_recoveries, 5);
        assert_eq!(config.watchdog.recovery_backoff_secs, 60);
    }

    #[test]
    fn test_load_config_rejects_auto_recover_max_zero() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "bad-recover"

[watchdog]
auto_recover = true
max_recoveries = 0
"#
        )
        .unwrap();

        let result = load(tmpfile.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max_recoveries"));
    }

    #[test]
    fn test_save_and_load_roundtrip_with_auto_recover() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("recover-rt.toml");
        let config = Config {
            node: NodeConfig {
                name: "recover-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig {
                auto_recover: true,
                max_recoveries: 5,
                recovery_backoff_secs: 60,
                ..WatchdogConfig::default()
            },
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert!(loaded.watchdog.auto_recover);
        assert_eq!(loaded.watchdog.max_recoveries, 5);
        assert_eq!(loaded.watchdog.recovery_backoff_secs, 60);
    }

    #[test]
    fn test_missing_config_has_default_auto_recover() {
        let config = load("/nonexistent/recover/config.toml").unwrap();
        assert!(!config.watchdog.auto_recover);
        assert_eq!(config.watchdog.max_recoveries, 3);
        assert_eq!(config.watchdog.recovery_backoff_secs, 30);
    }

    #[test]
    fn test_watchdog_validate_idle_action_alert() {
        let wd = WatchdogConfig {
            idle_action: "alert".into(),
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_watchdog_validate_idle_action_kill() {
        let wd = WatchdogConfig {
            idle_action: "kill".into(),
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_watchdog_validate_idle_action_invalid() {
        let wd = WatchdogConfig {
            idle_action: "pause".into(),
            ..WatchdogConfig::default()
        };
        let err = wd.validate().unwrap_err();
        assert!(err.to_string().contains("idle_action"));
    }

    #[test]
    fn test_load_config_with_idle_settings() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "idle-test"

[watchdog]
idle_timeout_secs = 300
idle_action = "kill"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.watchdog.idle_timeout_secs, 300);
        assert_eq!(config.watchdog.idle_action, "kill");
    }

    #[test]
    fn test_load_config_rejects_invalid_idle_action() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "bad-idle"

[watchdog]
idle_action = "pause"
"#
        )
        .unwrap();

        let result = load(tmpfile.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("idle_action"));
    }

    #[test]
    fn test_missing_config_has_default_idle() {
        let config = load("/nonexistent/idle/config.toml").unwrap();
        assert_eq!(config.watchdog.idle_timeout_secs, 600);
        assert_eq!(config.watchdog.idle_action, "alert");
    }

    #[test]
    fn test_save_and_load_roundtrip_with_idle() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("idle-rt.toml");
        let config = Config {
            node: NodeConfig {
                name: "idle-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig {
                idle_timeout_secs: 120,
                idle_action: "kill".into(),
                ..WatchdogConfig::default()
            },
            personas: HashMap::new(),
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.watchdog.idle_timeout_secs, 120);
        assert_eq!(loaded.watchdog.idle_action, "kill");
    }

    #[test]
    fn test_watchdog_idle_timeout_zero_disables() {
        let wd = WatchdogConfig {
            idle_timeout_secs: 0,
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_config_without_personas_section() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test"
port = 7433
data_dir = "/tmp"
"#
        )
        .unwrap();
        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(config.personas.is_empty());
    }

    #[test]
    fn test_config_with_personas_section() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test"
port = 7433
data_dir = "/tmp"

[personas.reviewer]
provider = "claude"
model = "sonnet"
mode = "autonomous"
guard_preset = "strict"
allowed_tools = ["Read", "Glob", "Grep"]
system_prompt = "You are a code reviewer."

[personas.coder]
model = "opus"
"#
        )
        .unwrap();
        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.personas.len(), 2);
        let reviewer = &config.personas["reviewer"];
        assert_eq!(reviewer.provider, Some("claude".into()));
        assert_eq!(reviewer.model, Some("sonnet".into()));
        assert_eq!(reviewer.mode, Some("autonomous".into()));
        assert_eq!(reviewer.guard_preset, Some("strict".into()));
        assert_eq!(
            reviewer.allowed_tools,
            Some(vec!["Read".into(), "Glob".into(), "Grep".into()])
        );
        assert_eq!(
            reviewer.system_prompt,
            Some("You are a code reviewer.".into())
        );
        let coder = &config.personas["coder"];
        assert_eq!(coder.provider, None);
        assert_eq!(coder.model, Some("opus".into()));
        assert!(coder.allowed_tools.is_none());
    }

    #[test]
    fn test_persona_config_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("persona-rt.toml");
        let mut personas = HashMap::new();
        personas.insert(
            "reviewer".into(),
            PersonaConfig {
                provider: Some("claude".into()),
                model: Some("sonnet".into()),
                mode: Some("autonomous".into()),
                guard_preset: Some("strict".into()),
                allowed_tools: Some(vec!["Read".into()]),
                system_prompt: Some("Review only.".into()),
                max_turns: None,
                max_budget_usd: None,
                output_format: None,
            },
        );
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas,
            notifications: NotificationsConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.personas.len(), 1);
        let reviewer = &loaded.personas["reviewer"];
        assert_eq!(reviewer.model, Some("sonnet".into()));
        assert_eq!(reviewer.system_prompt, Some("Review only.".into()));
    }

    #[test]
    fn test_persona_config_debug_clone() {
        let p = PersonaConfig {
            provider: Some("claude".into()),
            model: None,
            mode: None,
            guard_preset: None,
            allowed_tools: None,
            system_prompt: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
        };
        let cloned = p.clone();
        assert_eq!(format!("{p:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_config_without_notifications() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433
data_dir = "/tmp/test"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert!(config.notifications.discord.is_none());
    }

    #[test]
    fn test_config_with_discord_notifications() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433
data_dir = "/tmp/test"

[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/123/abc"
events = ["completed", "dead"]
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        let discord = config.notifications.discord.unwrap();
        assert_eq!(
            discord.webhook_url,
            "https://discord.com/api/webhooks/123/abc"
        );
        assert_eq!(discord.events, vec!["completed", "dead"]);
    }

    #[test]
    fn test_config_with_discord_no_filter() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433
data_dir = "/tmp/test"

[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/456/def"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        let discord = config.notifications.discord.unwrap();
        assert_eq!(
            discord.webhook_url,
            "https://discord.com/api/webhooks/456/def"
        );
        assert!(discord.events.is_empty());
    }

    #[test]
    fn test_notifications_config_save_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp/test".into(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            guards: GuardDefaultConfig::default(),
            watchdog: WatchdogConfig::default(),
            personas: HashMap::new(),
            notifications: NotificationsConfig {
                discord: Some(DiscordWebhookConfig {
                    webhook_url: "https://discord.com/api/webhooks/789/xyz".into(),
                    events: vec!["dead".into()],
                }),
            },
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        let discord = loaded.notifications.discord.unwrap();
        assert_eq!(
            discord.webhook_url,
            "https://discord.com/api/webhooks/789/xyz"
        );
        assert_eq!(discord.events, vec!["dead"]);
    }

    #[test]
    fn test_notifications_config_default() {
        let config = NotificationsConfig::default();
        assert!(config.discord.is_none());
    }

    #[test]
    fn test_notifications_config_debug_clone() {
        let config = NotificationsConfig {
            discord: Some(DiscordWebhookConfig {
                webhook_url: "url".into(),
                events: vec![],
            }),
        };
        let cloned = config.clone();
        assert_eq!(format!("{config:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_discord_webhook_config_debug_clone() {
        let config = DiscordWebhookConfig {
            webhook_url: "url".into(),
            events: vec!["dead".into()],
        };
        let cloned = config.clone();
        assert_eq!(format!("{config:?}"), format!("{cloned:?}"));
    }
}
