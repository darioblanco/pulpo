use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use pulpo_common::auth::BindMode;
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
    pub watchdog: WatchdogConfig,
    #[serde(default)]
    pub inks: HashMap<String, InkConfig>,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default, alias = "sandbox")]
    pub docker: DockerConfig,
    #[serde(default)]
    pub master: MasterConfig,
}

/// Master/worker mode configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MasterConfig {
    /// If true, this node aggregates events from workers.
    #[serde(default)]
    pub enabled: bool,
    /// URL of the master node (for workers). When set, this node pushes events to master.
    #[serde(default)]
    pub address: Option<String>,
    /// Bearer token for authenticating to master (workers use this).
    #[serde(default)]
    pub token: Option<String>,
    /// Seconds before master marks a silent worker's sessions as lost.
    #[serde(default = "default_stale_timeout")]
    pub stale_timeout_secs: u64,
}

impl Default for MasterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            address: None,
            token: None,
            stale_timeout_secs: default_stale_timeout(),
        }
    }
}

const fn default_stale_timeout() -> u64 {
    300
}

/// The role of a node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// Standalone node (default, no master/worker relationship).
    Standalone,
    /// Master node: aggregates events from workers.
    Master,
    /// Worker node: pushes events to a master.
    Worker,
}

/// Docker runtime configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DockerConfig {
    /// Docker image for container sessions.
    #[serde(default = "default_docker_image")]
    pub image: String,
    /// Volume mounts for Docker containers (host:container:mode format).
    /// Default includes agent auth directories (Claude, Codex, Gemini) as read-only.
    #[serde(default = "default_docker_volumes")]
    pub volumes: Vec<String>,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            image: default_docker_image(),
            volumes: default_docker_volumes(),
        }
    }
}

fn default_docker_image() -> String {
    "ubuntu:latest".into()
}

fn default_docker_volumes() -> Vec<String> {
    vec![
        "~/.claude:/root/.claude:ro".to_owned(),
        "~/.codex:/root/.codex:ro".to_owned(),
        "~/.gemini:/root/.gemini:ro".to_owned(),
    ]
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InkConfig {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    /// Secret names to inject as environment variables.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Runtime to use (tmux or docker). Overridden by --runtime on spawn.
    #[serde(default)]
    pub runtime: Option<String>,
}

/// Notification configuration (webhooks for status updates).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NotificationsConfig {
    /// Discord webhook notifications.
    #[serde(default)]
    pub discord: Option<DiscordWebhookConfig>,
    /// Generic webhook endpoints.
    #[serde(default)]
    pub webhooks: Vec<WebhookEndpointConfig>,
    /// VAPID keys for Web Push notifications.
    #[serde(default)]
    pub vapid: VapidConfig,
}

/// VAPID key configuration for Web Push notifications.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VapidConfig {
    /// Base64url-encoded P-256 private key (32 bytes).
    #[serde(default)]
    pub private_key: String,
    /// Base64url-encoded P-256 uncompressed public key (65 bytes).
    #[serde(default)]
    pub public_key: String,
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

/// Generic webhook endpoint configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebhookEndpointConfig {
    /// Human-readable name for this endpoint.
    pub name: String,
    /// URL to POST event payloads to.
    pub url: String,
    /// Optional event filter — only send for these statuses.
    /// If empty/absent, all events are sent.
    #[serde(default)]
    pub events: Vec<String>,
    /// Optional HMAC-SHA256 signing secret. When set, a `X-Pulpo-Signature`
    /// header is included with each request.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Authentication configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AuthConfig {
    /// Bearer token for API authentication (auto-generated on first run).
    /// Only used in `public` bind mode.
    #[serde(default)]
    pub token: String,
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

/// If VAPID keys are empty, generate a new P-256 key pair. Returns `true` if new keys were generated.
pub fn ensure_vapid_keys(config: &mut Config) -> bool {
    if config.notifications.vapid.private_key.is_empty()
        && config.notifications.vapid.public_key.is_empty()
    {
        let secret_key = p256::SecretKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
        let private_bytes = secret_key.to_bytes();
        let public_bytes = secret_key.public_key().to_sec1_bytes();

        config.notifications.vapid.private_key =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(private_bytes);
        config.notifications.vapid.public_key =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_bytes);
        true
    } else {
        false
    }
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
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_idle_action")]
    pub idle_action: String,
    /// Seconds after Ready before tmux shell is killed (0 = disabled).
    #[serde(default, alias = "finished_ttl_secs", alias = "exited_ttl_secs")]
    pub ready_ttl_secs: u64,
    /// Auto-adopt external tmux sessions into pulpo management.
    #[serde(default = "default_adopt_tmux")]
    pub adopt_tmux: bool,
    /// Seconds of unchanged output before Active→Idle transition (default: 60).
    #[serde(default = "default_idle_threshold_secs")]
    pub idle_threshold_secs: u64,
    /// Extra patterns that indicate the agent is waiting for user input.
    /// Appended to the built-in defaults.
    #[serde(default)]
    pub waiting_patterns: Vec<String>,
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
        if self.idle_threshold_secs == 0 {
            anyhow::bail!("watchdog.idle_threshold_secs must be >= 1");
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
            idle_timeout_secs: default_idle_timeout_secs(),
            idle_action: default_idle_action(),
            ready_ttl_secs: 0,
            adopt_tmux: default_adopt_tmux(),
            idle_threshold_secs: default_idle_threshold_secs(),
            waiting_patterns: Vec::new(),
        }
    }
}

const fn default_adopt_tmux() -> bool {
    true
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

const fn default_idle_timeout_secs() -> u64 {
    600
}

fn default_idle_action() -> String {
    String::from("alert")
}

const fn default_idle_threshold_secs() -> u64 {
    60
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeConfig {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    /// How the daemon binds to the network. Determines discovery method and auth requirements.
    #[serde(default)]
    pub bind: BindMode,
    /// Tailscale ACL tag to filter peers (e.g. `"pulpo"`). Only used with `tailscale` bind mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Scan interval in seconds for Tailscale peer discovery. Defaults to 30.
    #[serde(default = "default_discovery_interval_secs")]
    pub discovery_interval_secs: u64,
    /// Default command used when spawning a session without an explicit command or ink.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_command: Option<String>,
    /// Number of days to retain log files. Defaults to 7.
    #[serde(default = "default_log_retain_days")]
    pub log_retain_days: u32,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            name: default_name(),
            port: default_port(),
            data_dir: default_data_dir(),
            bind: BindMode::default(),
            tag: None,
            discovery_interval_secs: default_discovery_interval_secs(),
            default_command: None,
            log_retain_days: default_log_retain_days(),
        }
    }
}

const fn default_discovery_interval_secs() -> u64 {
    30
}

fn default_name() -> String {
    let fallback = String::from("unknown");
    hostname::get().map_or(fallback, |h| h.to_string_lossy().into_owned())
}

const fn default_port() -> u16 {
    7433
}

const fn default_log_retain_days() -> u32 {
    7
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

    /// Determine the node's role based on master configuration.
    pub const fn role(&self) -> NodeRole {
        if self.master.enabled {
            NodeRole::Master
        } else if self.master.address.is_some() {
            NodeRole::Worker
        } else {
            NodeRole::Standalone
        }
    }

    /// Validate the master configuration.
    ///
    /// - A node cannot be both master and worker (enabled + address).
    /// - In `public` bind mode, master mode requires `auth.token`.
    /// - In worker mode, `master.token` is always required.
    pub fn validate_master(&self) -> Result<()> {
        if self.master.enabled && self.master.address.is_some() {
            anyhow::bail!(
                "master.enabled and master.address are mutually exclusive: \
                 a node cannot be both master and worker"
            );
        }
        if self.master.enabled && self.node.bind == BindMode::Public {
            // Public master: must have auth.token so workers/users can authenticate.
            if self.auth.token.is_empty() {
                anyhow::bail!(
                    "master.enabled requires auth.token to be set: \
                     public master nodes require bearer auth for users and workers"
                );
            }
        }
        if self.master.address.is_some()
            && (self.master.token.is_none() || self.master.token.as_deref() == Some(""))
        {
            anyhow::bail!(
                "master.address requires master.token to be set: \
                 worker nodes require a bound bearer token to authenticate with the master"
            );
        }
        Ok(())
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

/// Built-in example inks for common engineering workflows.
/// User-defined inks with the same name override these defaults.
pub fn built_in_inks() -> HashMap<String, InkConfig> {
    let mut inks = HashMap::new();
    inks.insert(
        "reviewer".to_owned(),
        InkConfig {
            description: Some("Code review — read-only analysis, detailed feedback".to_owned()),
            command: Some(
                "claude -p 'Review this code for bugs, security issues, and style'".to_owned(),
            ),
            ..InkConfig::default()
        },
    );
    inks.insert(
        "coder".to_owned(),
        InkConfig {
            description: Some(
                "Implementation — full tool access, write production code".to_owned(),
            ),
            command: Some(
                "claude --dangerously-skip-permissions -p 'Implement the requested changes'"
                    .to_owned(),
            ),
            ..InkConfig::default()
        },
    );
    inks.insert(
        "quick-fix".to_owned(),
        InkConfig {
            description: Some(
                "Fast bug fixes — focused, small targeted changes".to_owned(),
            ),
            command: Some(
                "claude --dangerously-skip-permissions -p 'Fix the reported bug with minimal changes'"
                    .to_owned(),
            ),
            ..InkConfig::default()
        },
    );
    inks.insert(
        "tester".to_owned(),
        InkConfig {
            description: Some(
                "Test writing — generate comprehensive tests for existing code".to_owned(),
            ),
            command: Some(
                "claude -p 'Write comprehensive tests for the specified code'".to_owned(),
            ),
            ..InkConfig::default()
        },
    );
    inks.insert(
        "refactor".to_owned(),
        InkConfig {
            description: Some(
                "Refactoring — restructure code without changing behavior".to_owned(),
            ),
            command: Some(
                "claude --dangerously-skip-permissions -p 'Refactor the specified code'".to_owned(),
            ),
            ..InkConfig::default()
        },
    );
    inks
}

/// Merge built-in inks with user-defined inks. User inks override built-ins by name.
fn merge_built_in_inks(user_inks: HashMap<String, InkConfig>) -> HashMap<String, InkConfig> {
    let mut merged = built_in_inks();
    merged.extend(user_inks);
    merged
}

pub fn load(path: &str) -> Result<Config> {
    let expanded = shellexpand::tilde(path);
    let path = std::path::Path::new(expanded.as_ref());

    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let mut config: Config = toml::from_str(&content).context("Failed to parse config")?;
        config.watchdog.validate()?;
        config.validate_master()?;
        config.inks = merge_built_in_inks(config.inks);
        Ok(config)
    } else {
        // Return defaults if no config file exists
        Ok(Config {
            node: NodeConfig {
                name: default_name(),
                port: default_port(),
                data_dir: default_data_dir(),
                bind: BindMode::default(),
                tag: None,
                discovery_interval_secs: default_discovery_interval_secs(),
                default_command: None,
                log_retain_days: default_log_retain_days(),
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: built_in_inks(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
        // Built-in inks are included by default
        assert!(config.inks.contains_key("reviewer"));
        assert!(config.inks.contains_key("coder"));
        assert!(config.inks.contains_key("quick-fix"));
        assert!(config.inks.contains_key("tester"));
        assert!(config.inks.contains_key("refactor"));
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers,

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("node-a"));
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers,

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.name, "roundtrip");
        assert_eq!(loaded.node.port, 9000);
        assert_eq!(
            loaded.peers["remote"],
            PeerEntry::Simple("10.0.0.1:7433".into())
        );
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
                    ..NodeConfig::default()
                },
                auth: AuthConfig::default(),
                peers: HashMap::new(),

                watchdog: WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: NotificationsConfig::default(),
                docker: DockerConfig::default(),
                master: MasterConfig::default(),
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
                    ..NodeConfig::default()
                },
                auth: AuthConfig::default(),
                peers: HashMap::new(),

                watchdog: WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: NotificationsConfig::default(),
                docker: DockerConfig::default(),
                master: MasterConfig::default(),
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
                    ..NodeConfig::default()
                },
                auth: AuthConfig::default(),
                peers: HashMap::new(),

                watchdog: WatchdogConfig::default(),
                inks: HashMap::new(),
                notifications: NotificationsConfig::default(),
                docker: DockerConfig::default(),
                master: MasterConfig::default(),
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
            ..NodeConfig::default()
        };
        #[allow(clippy::redundant_clone)]
        let cloned = nc.clone();
        assert_eq!(cloned.name, "test");
    }

    #[test]
    fn test_config_serialize() {
        let config = Config {
            node: NodeConfig {
                name: "ser".into(),
                port: 1234,
                data_dir: "/d".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("ser"));
        assert!(toml_str.contains("1234"));
    }

    #[test]
    fn test_auth_config_default() {
        let auth = AuthConfig::default();
        assert!(auth.token.is_empty());
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
        };
        #[allow(clippy::redundant_clone)]
        let cloned = auth.clone();
        assert_eq!(cloned.token, "test-token");
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
                ..NodeConfig::default()
            },
            auth: AuthConfig {
                token: "existing-token".into(),
            },
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
bind = "public"

[auth]
token = "my-secret-token"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.auth.token, "my-secret-token");
        assert_eq!(config.node.bind, pulpo_common::auth::BindMode::Public);
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
                bind: pulpo_common::auth::BindMode::Public,
                ..NodeConfig::default()
            },
            auth: AuthConfig {
                token: "roundtrip-token".into(),
            },
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.auth.token, "roundtrip-token");
        assert_eq!(loaded.node.bind, pulpo_common::auth::BindMode::Public);
    }

    #[test]
    fn test_missing_config_has_default_auth() {
        let config = load("/nonexistent/auth/config.toml").unwrap();
        assert!(config.auth.token.is_empty());
        assert_eq!(config.node.bind, pulpo_common::auth::BindMode::Local);
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers,

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig {
                enabled: false,
                memory_threshold: 75,
                check_interval_secs: 30,
                breach_count: 5,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
                ready_ttl_secs: 0,
                adopt_tmux: true,
                idle_threshold_secs: 60,
                waiting_patterns: Vec::new(),
            },
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig {
                idle_timeout_secs: 120,
                idle_action: "kill".into(),
                ..WatchdogConfig::default()
            },
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.watchdog.idle_timeout_secs, 120);
        assert_eq!(loaded.watchdog.idle_action, "kill");
        assert_eq!(loaded.watchdog.idle_threshold_secs, 60);
        assert!(loaded.watchdog.waiting_patterns.is_empty());
    }

    #[test]
    fn test_validate_idle_threshold_secs_zero() {
        let cfg = WatchdogConfig {
            idle_threshold_secs: 0,
            ..WatchdogConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_default_idle_threshold_secs() {
        let cfg = WatchdogConfig::default();
        assert_eq!(cfg.idle_threshold_secs, 60);
    }

    #[test]
    fn test_default_waiting_patterns() {
        let cfg = WatchdogConfig::default();
        assert!(cfg.waiting_patterns.is_empty());
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
    fn test_config_without_inks_section() {
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
        // Built-in inks are always present even without [inks] section
        assert_eq!(config.inks.len(), 5);
        assert!(config.inks.contains_key("reviewer"));
    }

    #[test]
    fn test_config_with_inks_section() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test"
port = 7433
data_dir = "/tmp"

[inks.reviewer]
command = "claude -p 'Custom review'"
description = "Code review specialist"

[inks.coder]
command = "codex -p 'Do it'"
"#
        )
        .unwrap();
        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        // 5 built-in inks + user overrides (reviewer and coder are overrides, so still 5)
        assert_eq!(config.inks.len(), 5);
        // User config overrides the built-in reviewer
        let reviewer = &config.inks["reviewer"];
        assert_eq!(reviewer.command, Some("claude -p 'Custom review'".into()));
        assert_eq!(reviewer.description, Some("Code review specialist".into()));
        // User config overrides the built-in coder (partial — only command set)
        let coder = &config.inks["coder"];
        assert_eq!(coder.command, Some("codex -p 'Do it'".into()));
        assert!(coder.description.is_none());
    }

    #[test]
    fn test_ink_config_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("ink-rt.toml");
        let mut inks = HashMap::new();
        inks.insert(
            "reviewer".into(),
            InkConfig {
                description: Some("Code reviewer".into()),
                command: Some("claude -p 'Review only'".into()),
                ..InkConfig::default()
            },
        );
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks,
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        // 1 user ink + 4 other built-ins (reviewer is overridden)
        assert_eq!(loaded.inks.len(), 5);
        let reviewer = &loaded.inks["reviewer"];
        assert_eq!(reviewer.command, Some("claude -p 'Review only'".into()));
        assert_eq!(reviewer.description, Some("Code reviewer".into()));
    }

    #[test]
    fn test_ink_config_debug_clone() {
        let p = InkConfig {
            description: None,
            command: Some("claude -p 'test'".into()),
            ..InkConfig::default()
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
events = ["ready", "killed"]
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        let discord = config.notifications.discord.unwrap();
        assert_eq!(
            discord.webhook_url,
            "https://discord.com/api/webhooks/123/abc"
        );
        assert_eq!(discord.events, vec!["ready", "killed"]);
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
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig {
                discord: Some(DiscordWebhookConfig {
                    webhook_url: "https://discord.com/api/webhooks/789/xyz".into(),
                    events: vec!["killed".into()],
                }),
                webhooks: vec![],
                ..Default::default()
            },
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        let discord = loaded.notifications.discord.unwrap();
        assert_eq!(
            discord.webhook_url,
            "https://discord.com/api/webhooks/789/xyz"
        );
        assert_eq!(discord.events, vec!["killed"]);
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
            webhooks: vec![],
            ..Default::default()
        };
        let cloned = config.clone();
        assert_eq!(format!("{config:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_discord_webhook_config_debug_clone() {
        let config = DiscordWebhookConfig {
            webhook_url: "url".into(),
            events: vec!["killed".into()],
        };
        let cloned = config.clone();
        assert_eq!(format!("{config:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_notifications_config_default_webhooks_empty() {
        let config = NotificationsConfig::default();
        assert!(config.webhooks.is_empty());
    }

    #[test]
    fn test_webhook_endpoint_config_debug_clone() {
        let config = WebhookEndpointConfig {
            name: "hook".into(),
            url: "https://example.com".into(),
            events: vec!["killed".into()],
            secret: Some("key".into()),
        };
        let cloned = config.clone();
        assert_eq!(format!("{config:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_webhook_endpoint_config_serde_roundtrip() {
        let config = WebhookEndpointConfig {
            name: "ci".into(),
            url: "https://ci.example.com/hook".into(),
            events: vec!["ready".into()],
            secret: Some("s3cret".into()),
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: WebhookEndpointConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.name, "ci");
        assert_eq!(parsed.url, "https://ci.example.com/hook");
        assert_eq!(parsed.events, vec!["ready"]);
        assert_eq!(parsed.secret, Some("s3cret".into()));
    }

    #[test]
    fn test_webhook_endpoint_config_no_secret() {
        let toml_str = r#"
name = "hook"
url = "https://example.com"
"#;
        let parsed: WebhookEndpointConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.secret.is_none());
        assert!(parsed.events.is_empty());
    }

    #[test]
    fn test_config_roundtrip_with_webhooks() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        let config = Config {
            node: NodeConfig::default(),
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig {
                discord: None,
                webhooks: vec![WebhookEndpointConfig {
                    name: "test-hook".into(),
                    url: "https://example.com/hook".into(),
                    events: vec!["killed".into()],
                    secret: Some("key".into()),
                }],
                ..Default::default()
            },
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.notifications.webhooks.len(), 1);
        let wh = &loaded.notifications.webhooks[0];
        assert_eq!(wh.name, "test-hook");
        assert_eq!(wh.url, "https://example.com/hook");
        assert_eq!(wh.events, vec!["killed"]);
        assert_eq!(wh.secret, Some("key".into()));
    }

    // -- Node bind/discovery config tests --

    #[test]
    fn test_node_config_default() {
        let node = NodeConfig::default();
        assert!(!node.name.is_empty());
        assert_eq!(node.port, 7433);
        assert_eq!(node.bind, pulpo_common::auth::BindMode::Local);
        assert!(node.tag.is_none());
        assert_eq!(node.discovery_interval_secs, 30);
    }

    #[test]
    fn test_load_config_with_tailscale_bind() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
bind = "tailscale"
tag = "pulpo"
discovery_interval_secs = 60
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.node.bind, pulpo_common::auth::BindMode::Tailscale);
        assert_eq!(config.node.tag, Some("pulpo".into()));
        assert_eq!(config.node.discovery_interval_secs, 60);
    }

    #[test]
    fn test_load_config_without_bind_defaults_to_local() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.node.bind, pulpo_common::auth::BindMode::Local);
        assert!(config.node.tag.is_none());
    }

    #[test]
    fn test_save_and_load_config_with_tailscale() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp/test".into(),
                bind: pulpo_common::auth::BindMode::Tailscale,
                tag: Some("my-tag".into()),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),

            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.bind, pulpo_common::auth::BindMode::Tailscale);
        assert_eq!(loaded.node.tag, Some("my-tag".into()));
    }

    #[test]
    fn test_node_config_tag_skip_serializing_if_none() {
        let config = NodeConfig {
            name: "test".into(),
            port: 7433,
            data_dir: "/tmp".into(),
            bind: pulpo_common::auth::BindMode::Public,
            tag: None,
            discovery_interval_secs: 30,
            default_command: None,
            log_retain_days: 7,
        };
        let toml_str = toml::to_string(&config).unwrap();
        // tag should be skipped (None + skip_serializing_if)
        assert!(!toml_str.contains("tag"));
    }

    #[test]
    fn test_built_in_inks_contains_expected_keys() {
        let inks = built_in_inks();
        assert!(inks.contains_key("reviewer"));
        assert!(inks.contains_key("coder"));
        assert!(inks.contains_key("quick-fix"));
        assert!(inks.contains_key("tester"));
        assert!(inks.contains_key("refactor"));
        assert_eq!(inks.len(), 5);
    }

    #[test]
    fn test_built_in_inks_have_descriptions() {
        let inks = built_in_inks();
        for (name, ink) in &inks {
            assert!(
                ink.description.is_some(),
                "Built-in ink '{name}' missing description"
            );
        }
    }

    #[test]
    fn test_built_in_inks_have_commands() {
        let inks = built_in_inks();
        for (name, ink) in &inks {
            assert!(
                ink.command.is_some(),
                "Built-in ink '{name}' missing command"
            );
        }
    }

    #[test]
    fn test_merge_built_in_inks_user_overrides() {
        let mut user_inks = HashMap::new();
        user_inks.insert(
            "reviewer".to_owned(),
            InkConfig {
                description: Some("My custom reviewer".to_owned()),
                command: Some("codex -p 'review'".to_owned()),
                ..InkConfig::default()
            },
        );
        let merged = merge_built_in_inks(user_inks);
        // User's reviewer overrides built-in
        assert_eq!(
            merged["reviewer"].description.as_deref(),
            Some("My custom reviewer")
        );
        assert_eq!(
            merged["reviewer"].command.as_deref(),
            Some("codex -p 'review'")
        );
        // Other built-ins still present
        assert!(merged.contains_key("coder"));
        assert!(merged.contains_key("quick-fix"));
    }

    #[test]
    fn test_merge_built_in_inks_user_adds_new() {
        let mut user_inks = HashMap::new();
        user_inks.insert(
            "my-custom".to_owned(),
            InkConfig {
                description: Some("Custom ink".to_owned()),
                command: None,
                ..InkConfig::default()
            },
        );
        let merged = merge_built_in_inks(user_inks);
        // User ink is added alongside built-ins
        assert!(merged.contains_key("my-custom"));
        assert!(merged.contains_key("reviewer"));
        assert_eq!(merged.len(), 6);
    }

    #[test]
    fn test_load_config_merges_built_in_inks() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test-node"
port = 7433
data_dir = "/tmp/pulpo-test"

[inks.reviewer]
description = "Overridden reviewer"
command = "codex -p 'review'"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        // User override wins
        assert_eq!(
            config.inks["reviewer"].description.as_deref(),
            Some("Overridden reviewer")
        );
        assert_eq!(
            config.inks["reviewer"].command.as_deref(),
            Some("codex -p 'review'")
        );
        // Built-in inks still present
        assert!(config.inks.contains_key("coder"));
        assert!(config.inks.contains_key("quick-fix"));
        assert!(config.inks.contains_key("tester"));
        assert!(config.inks.contains_key("refactor"));
    }

    #[test]
    fn test_ink_config_description_serialization() {
        let ink = InkConfig {
            description: Some("Test description".to_owned()),
            command: Some("claude -p 'test'".to_owned()),
            ..InkConfig::default()
        };
        let toml_str = toml::to_string(&ink).unwrap();
        assert!(toml_str.contains("description = \"Test description\""));
        let deserialized: InkConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(
            deserialized.description.as_deref(),
            Some("Test description")
        );
    }

    #[test]
    fn test_ink_config_command_default_none() {
        let ink: InkConfig = toml::from_str("").unwrap();
        assert!(ink.command.is_none());
    }

    #[test]
    fn test_ink_config_secrets_and_runtime() {
        let toml_str = r#"
            command = "claude"
            secrets = ["GITHUB_TOKEN", "NPM_TOKEN"]
            runtime = "docker"
        "#;
        let ink: InkConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(ink.secrets, vec!["GITHUB_TOKEN", "NPM_TOKEN"]);
        assert_eq!(ink.runtime.as_deref(), Some("docker"));
    }

    #[test]
    fn test_ink_config_secrets_default_empty() {
        let ink: InkConfig = toml::from_str("").unwrap();
        assert!(ink.secrets.is_empty());
        assert!(ink.runtime.is_none());
    }

    // -- VAPID key generation tests --

    #[test]
    fn test_vapid_config_default() {
        let vapid = VapidConfig::default();
        assert!(vapid.private_key.is_empty());
        assert!(vapid.public_key.is_empty());
    }

    #[test]
    fn test_vapid_config_debug_clone() {
        let vapid = VapidConfig {
            private_key: "priv".into(),
            public_key: "pub".into(),
        };
        let cloned = vapid.clone();
        assert_eq!(format!("{vapid:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_ensure_vapid_keys_generates_when_empty() {
        let mut config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        assert!(config.notifications.vapid.private_key.is_empty());
        assert!(config.notifications.vapid.public_key.is_empty());

        let generated = ensure_vapid_keys(&mut config);
        assert!(generated);
        assert!(!config.notifications.vapid.private_key.is_empty());
        assert!(!config.notifications.vapid.public_key.is_empty());
    }

    #[test]
    fn test_ensure_vapid_keys_correct_lengths() {
        let mut config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        ensure_vapid_keys(&mut config);

        // Private key: 32 bytes → 43 chars base64url (no padding)
        assert_eq!(config.notifications.vapid.private_key.len(), 43);
        // Public key: 65 bytes → 87 chars base64url (no padding)
        assert_eq!(config.notifications.vapid.public_key.len(), 87);
    }

    #[test]
    fn test_ensure_vapid_keys_are_base64url() {
        let mut config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        ensure_vapid_keys(&mut config);

        for key in [
            &config.notifications.vapid.private_key,
            &config.notifications.vapid.public_key,
        ] {
            assert!(
                key.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "Key should be base64url: {key}"
            );
        }
    }

    #[test]
    fn test_ensure_vapid_keys_preserves_existing() {
        let mut config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig {
                vapid: VapidConfig {
                    private_key: "existing-private".into(),
                    public_key: "existing-public".into(),
                },
                ..Default::default()
            },
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        let generated = ensure_vapid_keys(&mut config);
        assert!(!generated);
        assert_eq!(config.notifications.vapid.private_key, "existing-private");
        assert_eq!(config.notifications.vapid.public_key, "existing-public");
    }

    #[test]
    fn test_ensure_vapid_keys_uniqueness() {
        let mut config1 = Config {
            node: NodeConfig::default(),
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        let mut config2 = config1.clone();
        ensure_vapid_keys(&mut config1);
        ensure_vapid_keys(&mut config2);
        assert_ne!(
            config1.notifications.vapid.private_key,
            config2.notifications.vapid.private_key
        );
    }

    #[test]
    fn test_vapid_keys_save_and_load_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("vapid-rt.toml");
        let mut config = Config {
            node: NodeConfig {
                name: "vapid-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        ensure_vapid_keys(&mut config);
        let private_key = config.notifications.vapid.private_key.clone();
        let public_key = config.notifications.vapid.public_key.clone();

        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.notifications.vapid.private_key, private_key);
        assert_eq!(loaded.notifications.vapid.public_key, public_key);
    }

    #[test]
    fn test_notifications_config_default_has_empty_vapid() {
        let config = NotificationsConfig::default();
        assert!(config.vapid.private_key.is_empty());
        assert!(config.vapid.public_key.is_empty());
    }

    #[test]
    fn test_load_config_with_default_command() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test"
default_command = "claude"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.default_command, Some("claude".into()));
    }

    #[test]
    fn test_load_config_without_default_command() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.default_command, None);
    }

    #[test]
    fn test_save_config_with_default_command() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                default_command: Some("claude".into()),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.default_command, Some("claude".into()));
    }

    #[test]
    fn test_save_config_without_default_command_omits_field() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("default_command"),
            "None should be omitted from serialized config"
        );
    }

    #[test]
    fn test_docker_config_default() {
        let config = DockerConfig::default();
        assert_eq!(config.image, "ubuntu:latest");
    }

    #[test]
    fn test_docker_config_debug_clone() {
        let config = DockerConfig::default();
        let debug = format!("{config:?}");
        assert!(debug.contains("ubuntu:latest"));
        #[allow(clippy::redundant_clone)]
        let cloned = config.clone();
        assert_eq!(cloned.image, "ubuntu:latest");
    }

    #[test]
    fn test_load_config_with_docker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[docker]
image = "my-custom-image:v1"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.docker.image, "my-custom-image:v1");
    }

    #[test]
    fn test_load_config_with_sandbox_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[sandbox]
image = "legacy-image:v1"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.docker.image, "legacy-image:v1");
    }

    #[test]
    fn test_load_config_without_docker_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.docker.image, "ubuntu:latest");
    }

    #[test]
    fn test_default_docker_image_fn() {
        assert_eq!(default_docker_image(), "ubuntu:latest");
    }

    #[test]
    fn test_default_docker_volumes() {
        let volumes = default_docker_volumes();
        assert_eq!(volumes.len(), 3);
        assert_eq!(volumes[0], "~/.claude:/root/.claude:ro");
        assert_eq!(volumes[1], "~/.codex:/root/.codex:ro");
        assert_eq!(volumes[2], "~/.gemini:/root/.gemini:ro");
    }

    #[test]
    fn test_docker_config_default_has_volumes() {
        let config = DockerConfig::default();
        assert_eq!(config.volumes.len(), 3);
        assert!(config.volumes[0].contains(".claude"));
    }

    #[test]
    fn test_load_config_with_custom_docker_volumes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[docker]
image = "my-image:v1"
volumes = [
    "~/.ssh:/root/.ssh:ro",
    "~/.gitconfig:/root/.gitconfig:ro",
]
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.docker.image, "my-image:v1");
        assert_eq!(config.docker.volumes.len(), 2);
        assert_eq!(config.docker.volumes[0], "~/.ssh:/root/.ssh:ro");
        assert_eq!(config.docker.volumes[1], "~/.gitconfig:/root/.gitconfig:ro");
    }

    #[test]
    fn test_load_config_with_empty_docker_volumes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[docker]
volumes = []
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert!(config.docker.volumes.is_empty());
    }

    #[test]
    fn test_load_config_without_docker_volumes_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[docker]
image = "my-image:v1"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.docker.volumes.len(), 3);
        assert!(config.docker.volumes[0].contains(".claude"));
    }

    #[test]
    fn test_ink_config_with_secrets_and_runtime_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("ink-secrets-rt.toml");
        let mut inks = HashMap::new();
        inks.insert(
            "my-ink".into(),
            InkConfig {
                description: Some("Custom ink with secrets".into()),
                command: Some("claude -p 'build'".into()),
                secrets: vec!["GITHUB_TOKEN".into(), "NPM_TOKEN".into()],
                runtime: Some("docker".into()),
            },
        );
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks,
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        let ink = &loaded.inks["my-ink"];
        assert_eq!(ink.secrets, vec!["GITHUB_TOKEN", "NPM_TOKEN"]);
        assert_eq!(ink.runtime.as_deref(), Some("docker"));
        assert_eq!(ink.command.as_deref(), Some("claude -p 'build'"));
    }

    #[test]
    fn test_load_old_config_without_docker_section_backward_compat() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "old-node"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        // Docker config should have defaults
        assert_eq!(config.docker.image, "ubuntu:latest");
        assert_eq!(config.docker.volumes.len(), 3);
    }

    #[test]
    fn test_docker_volumes_save_roundtrip() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        let config = Config {
            node: NodeConfig {
                name: "test".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig {
                image: "my-image:v1".into(),
                volumes: vec!["~/.ssh:/root/.ssh:ro".into()],
            },
            master: MasterConfig::default(),
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.docker.volumes.len(), 1);
        assert_eq!(loaded.docker.volumes[0], "~/.ssh:/root/.ssh:ro");
    }

    // -- Master mode config tests --

    #[test]
    fn test_master_config_default() {
        let mc = MasterConfig::default();
        assert!(!mc.enabled);
        assert!(mc.address.is_none());
        assert!(mc.token.is_none());
        assert_eq!(mc.stale_timeout_secs, 300);
    }

    #[test]
    fn test_master_config_debug_clone() {
        let mc = MasterConfig {
            enabled: true,
            address: Some("http://master:7433".into()),
            token: Some("tok".into()),
            stale_timeout_secs: 600,
        };
        let cloned = mc.clone();
        assert_eq!(format!("{mc:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_config_no_master_section_role_standalone() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "standalone"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.role(), NodeRole::Standalone);
    }

    #[test]
    fn test_config_master_enabled_role_master() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "master-node"
port = 7433

[auth]
token = "master-token"

[master]
enabled = true
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.role(), NodeRole::Master);
        assert!(config.master.enabled);
    }

    #[test]
    fn test_config_master_address_role_worker() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "worker-node"
port = 7433

[master]
address = "http://master:7433"
token = "worker-token"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.role(), NodeRole::Worker);
        assert_eq!(config.master.address.as_deref(), Some("http://master:7433"));
        assert_eq!(config.master.token.as_deref(), Some("worker-token"));
    }

    #[test]
    fn test_config_master_enabled_and_address_validation_error() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "invalid"
port = 7433

[master]
enabled = true
address = "http://master:7433"
"#
        )
        .unwrap();

        let result = load(tmpfile.path().to_str().unwrap());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("mutually exclusive"),
            "Expected 'mutually exclusive' in error: {err}"
        );
    }

    #[test]
    fn test_default_stale_timeout_value() {
        assert_eq!(default_stale_timeout(), 300);
    }

    #[test]
    fn test_master_config_custom_stale_timeout() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "master"
port = 7433

[auth]
token = "master-token"

[master]
enabled = true
stale_timeout_secs = 600
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.master.stale_timeout_secs, 600);
    }

    #[test]
    fn test_master_config_default_stale_timeout_from_serde() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "master"
port = 7433

[auth]
token = "master-token"

[master]
enabled = true
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.master.stale_timeout_secs, 300);
    }

    #[test]
    fn test_missing_config_has_default_master() {
        let config = load("/nonexistent/master/config.toml").unwrap();
        assert!(!config.master.enabled);
        assert!(config.master.address.is_none());
        assert!(config.master.token.is_none());
        assert_eq!(config.master.stale_timeout_secs, 300);
        assert_eq!(config.role(), NodeRole::Standalone);
    }

    #[test]
    fn test_save_and_load_roundtrip_with_master() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("master-rt.toml");
        let config = Config {
            node: NodeConfig {
                name: "master-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            auth: AuthConfig {
                token: "master-auth-token".into(),
            },
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: true,
                address: None,
                token: Some("master-token".into()),
                stale_timeout_secs: 120,
            },
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert!(loaded.master.enabled);
        assert!(loaded.master.address.is_none());
        assert_eq!(loaded.master.token.as_deref(), Some("master-token"));
        assert_eq!(loaded.master.stale_timeout_secs, 120);
        assert_eq!(loaded.role(), NodeRole::Master);
    }

    #[test]
    fn test_node_role_debug() {
        assert_eq!(format!("{:?}", NodeRole::Standalone), "Standalone");
        assert_eq!(format!("{:?}", NodeRole::Master), "Master");
        assert_eq!(format!("{:?}", NodeRole::Worker), "Worker");
    }

    #[test]
    fn test_node_role_clone_copy_eq() {
        let role = NodeRole::Master;
        #[allow(clippy::clone_on_copy)]
        let cloned = role.clone();
        assert_eq!(role, cloned);

        let role2 = NodeRole::Worker;
        assert_ne!(role, role2);
    }

    #[test]
    fn test_validate_master_standalone_ok() {
        let config = Config {
            node: NodeConfig::default(),
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig::default(),
        };
        assert!(config.validate_master().is_ok());
    }

    #[test]
    fn test_validate_master_enabled_ok() {
        let config = Config {
            node: NodeConfig::default(),
            auth: AuthConfig {
                token: "secret-token".into(),
            },
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: true,
                address: None,
                ..MasterConfig::default()
            },
        };
        assert!(config.validate_master().is_ok());
    }

    #[test]
    fn test_validate_master_worker_ok() {
        let config = Config {
            node: NodeConfig::default(),
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: false,
                address: Some("http://master:7433".into()),
                token: Some("worker-token".into()),
                ..MasterConfig::default()
            },
        };
        assert!(config.validate_master().is_ok());
    }

    #[test]
    fn test_validate_master_both_enabled_and_address_errors() {
        let config = Config {
            node: NodeConfig::default(),
            auth: AuthConfig {
                token: "secret-token".into(),
            },
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: true,
                address: Some("http://master:7433".into()),
                ..MasterConfig::default()
            },
        };
        let err = config.validate_master().unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn test_validate_master_enabled_without_auth_token_errors() {
        let config = Config {
            node: NodeConfig {
                bind: BindMode::Public,
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(), // token is empty
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: true,
                address: None,
                ..MasterConfig::default()
            },
        };
        let err = config.validate_master().unwrap_err();
        assert!(err.to_string().contains("auth.token"));
    }

    #[test]
    fn test_validate_master_enabled_without_auth_token_ok_on_tailscale() {
        let config = Config {
            node: NodeConfig {
                bind: BindMode::Tailscale,
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: true,
                address: None,
                ..MasterConfig::default()
            },
        };
        assert!(config.validate_master().is_ok());
    }

    #[test]
    fn test_validate_master_worker_without_master_token_errors() {
        let config = Config {
            node: NodeConfig {
                bind: BindMode::Public,
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: false,
                address: Some("http://master:7433".into()),
                token: None, // missing
                ..MasterConfig::default()
            },
        };
        let err = config.validate_master().unwrap_err();
        assert!(err.to_string().contains("master.token"));
    }

    #[test]
    fn test_validate_master_worker_without_master_token_errors_on_tailscale() {
        let config = Config {
            node: NodeConfig {
                bind: BindMode::Tailscale,
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: false,
                address: Some("https://master.tailnet.ts.net".into()),
                token: None,
                ..MasterConfig::default()
            },
        };
        let err = config.validate_master().unwrap_err();
        assert!(err.to_string().contains("master.token"));
    }

    #[test]
    fn test_validate_master_worker_with_empty_master_token_errors() {
        let config = Config {
            node: NodeConfig {
                bind: BindMode::Public,
                ..NodeConfig::default()
            },
            auth: AuthConfig::default(),
            peers: HashMap::new(),
            watchdog: WatchdogConfig::default(),
            inks: HashMap::new(),
            notifications: NotificationsConfig::default(),
            docker: DockerConfig::default(),
            master: MasterConfig {
                enabled: false,
                address: Some("http://master:7433".into()),
                token: Some(String::new()), // empty string
                ..MasterConfig::default()
            },
        };
        let err = config.validate_master().unwrap_err();
        assert!(err.to_string().contains("master.token"));
    }

    #[test]
    fn test_load_old_config_without_master_section_backward_compat() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "old-node"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        // Master config should have defaults
        assert!(!config.master.enabled);
        assert!(config.master.address.is_none());
        assert_eq!(config.role(), NodeRole::Standalone);
    }
}
