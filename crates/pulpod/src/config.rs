use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use pulpo_common::auth::BindMode;
use pulpo_common::peer::PeerEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub node: NodeConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub peers: HashMap<String, PeerEntry>,
    #[serde(default)]
    pub watchdog: WatchdogConfig,
    #[serde(default)]
    pub plans: HashMap<String, PlanConfig>,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    /// Canonical top-level `[[webhooks]]` endpoints.
    ///
    /// Each endpoint filters the universal event stream by `events`
    /// (`<type>.<subtype>` globs) and `min_severity`. This is the supported
    /// location; the legacy `[notifications.webhooks]` form is still read and
    /// unioned with this list at startup for back-compat.
    #[serde(default)]
    pub webhooks: Vec<WebhookEndpointConfig>,
    /// Retired `[docker]` session-runtime configuration.
    /// The docker session runtime was removed — this field only exists so
    /// configs written before the removal still load (`deny_unknown_fields`
    /// would otherwise reject them). It is ignored and dropped on save.
    #[serde(default, skip_serializing)]
    pub docker: Option<toml::Value>,
    /// Retired `[controller]` mode configuration.
    /// Controller/node relay mode was removed — every pulpod is standalone and
    /// reached directly (peer registry + Tailscale). This field only exists so
    /// configs written before the removal still load (`deny_unknown_fields`
    /// would otherwise reject them). It is ignored and dropped on save.
    #[serde(default, skip_serializing)]
    pub controller: Option<toml::Value>,
    /// Retired `[inks.<name>]` preset registry configuration.
    /// Inks were removed — command/secrets/runtime live directly on sessions and
    /// schedules, and budgets moved onto schedules. This field only exists so
    /// configs written before the removal still load (`deny_unknown_fields`
    /// would otherwise reject them). It is ignored and dropped on save.
    #[serde(default, skip_serializing)]
    pub inks: Option<toml::Value>,
    #[serde(default)]
    pub metrics: MetricsConfig,
    /// Per-model cost rates, keyed by a model-ID substring (`[rates.<model>]`).
    ///
    /// Overrides — or adds — entries in the built-in rate table so a new or repriced
    /// model is metered correctly without a code change. Keys match the model ID
    /// case-insensitively by substring; the most specific (longest) match wins and
    /// any override beats the built-in table. Pulpo stays model-agnostic: a model
    /// with neither a built-in rate nor an override still reports exact tokens, with
    /// cost withheld rather than guessed.
    #[serde(default)]
    pub rates: HashMap<String, RateConfig>,
}

/// One `[rates.<model>]` entry: USD per million tokens.
///
/// `input` and `output` are required; the cache fields default to `0.0` when omitted
/// (correct for models without prompt caching, and a safe under-count for a quick reprice).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RateConfig {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write_5m: f64,
    #[serde(default)]
    pub cache_write_1h: f64,
}

/// Prometheus `/metrics` endpoint configuration.
///
/// Off by default. When enabled, `GET /api/v1/metrics` serves pull-based,
/// stateless Prometheus text — every gauge is computed from the current store
/// state on each scrape; nothing is persisted.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
    /// When `true`, the `/metrics` endpoint is served. Defaults to `false`.
    #[serde(default)]
    pub enabled: bool,
}

/// Per-plan quota configuration.
///
/// Anthropic does not publish subscription token allowances, so Claude "% of weekly
/// cap" / time-to-cap projections are computed only when the operator supplies an
/// estimate here. Keyed by the plan name reported in session `auth_plan` metadata
/// (e.g. `max`, `pro`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanConfig {
    /// Estimated weekly token allowance for this plan. `None` disables %-of-cap.
    #[serde(default)]
    pub weekly_token_allowance: Option<u64>,
}

/// Notification configuration (webhooks for status updates).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationsConfig {
    /// Retired `[notifications.discord]` webhook notifier configuration.
    /// The Discord webhook notifier was removed — this field only exists so
    /// configs written before the removal still load (`deny_unknown_fields`
    /// would otherwise reject them). It is ignored and dropped on save.
    #[serde(default, skip_serializing)]
    pub discord: Option<toml::Value>,
    /// Generic webhook endpoints.
    ///
    /// **Deprecated location.** Prefer the canonical top-level `[[webhooks]]`
    /// table on [`Config`]. This nested form is still read for back-compat and
    /// unioned with the top-level list at startup, so configs written before the
    /// promotion keep working unchanged.
    #[serde(default)]
    pub webhooks: Vec<WebhookEndpointConfig>,
    /// VAPID keys for Web Push notifications.
    #[serde(default)]
    pub vapid: VapidConfig,
}

/// VAPID key configuration for Web Push notifications.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VapidConfig {
    /// Base64url-encoded P-256 private key (32 bytes).
    #[serde(default)]
    pub private_key: String,
    /// Base64url-encoded P-256 uncompressed public key (65 bytes).
    #[serde(default)]
    pub public_key: String,
    /// Base64url-encoded 256-bit HMAC secret for signing push action tokens
    /// (the "Stop session" capability embedded in `usage_alert` push payloads).
    /// Stored alongside the VAPID keys and generated the same way — a fresh
    /// server-side secret on first run, auto-backfilled for configs that
    /// already have VAPID keys but predate this field. Never exposed over the
    /// API (unlike `public_key`, which `GET /api/v1/push/vapid-key` serves).
    #[serde(default)]
    pub action_secret: String,
}

/// Generic webhook endpoint configuration.
///
/// An endpoint subscribes to the universal event stream and receives every
/// canonical [`Event`](pulpo_common::event::Event) whose `<type>.<subtype>`
/// matches one of its `events` globs and whose `severity` is at or above
/// `min_severity`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookEndpointConfig {
    /// Human-readable name for this endpoint. Used to resolve outbox rows back
    /// to their endpoint, so it must be unique across configured webhooks.
    pub name: String,
    /// URL to POST event payloads to.
    pub url: String,
    /// Event filter — glob patterns matched against `"<type>.<subtype>"`
    /// (e.g. `"lifecycle.idle"`, `"usage_alert.*"`, `"intervention.*"`).
    ///
    /// Supported forms: an exact match (`lifecycle.idle`), a prefix glob
    /// (`lifecycle.*`), a bare type (`lifecycle`, matching every subtype of that
    /// type), and `*` (everything). An empty/absent list matches all events.
    #[serde(default)]
    pub events: Vec<String>,
    /// Minimum severity to deliver, ordered `info` < `warn` < `critical`.
    /// Events below this floor are dropped. Absent ⇒ no floor (all severities).
    #[serde(default)]
    pub min_severity: Option<String>,
    /// Optional HMAC-SHA256 signing secret. When set, a `X-Pulpo-Signature`
    /// header is included with each request.
    #[serde(default)]
    pub secret: Option<String>,
}

/// Match a single `events` glob pattern against an `"<type>.<subtype>"` event key.
///
/// Supported pattern forms (see [`WebhookEndpointConfig::events`]):
/// - `*` — matches everything.
/// - `lifecycle.*` — prefix glob: matches any `lifecycle.<subtype>`.
/// - `lifecycle` — bare type: matches any `lifecycle.<subtype>` (and the bare
///   `lifecycle` key itself, defensively).
/// - `lifecycle.idle` — exact match.
///
/// Deliberately tiny: only the trailing-`*` and bare-type shapes the contract
/// uses, so we avoid pulling in a glob crate.
pub fn glob_match(pattern: &str, event_key: &str) -> bool {
    if pattern == "*" || pattern == event_key {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        // `lifecycle.*` matches `lifecycle.<anything>`.
        return event_key
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'));
    }
    if !pattern.contains('.') {
        // Bare type, e.g. `lifecycle` ⇒ matches `lifecycle.<anything>`.
        return event_key
            .strip_prefix(pattern)
            .is_some_and(|rest| rest.starts_with('.'));
    }
    false
}

/// Numeric rank for a severity string, ordered `info` < `warn` < `critical`.
///
/// Unknown severities sort lowest (rank 0) so they are never dropped by a floor
/// they cannot be compared against.
const fn severity_rank(severity: &str) -> u8 {
    match severity.as_bytes() {
        b"critical" => 2,
        b"warn" => 1,
        _ => 0,
    }
}

/// Whether `severity` clears the optional `min_severity` floor.
///
/// `None` floor admits every severity. Otherwise the event's severity must rank
/// at or above the floor (`info` < `warn` < `critical`). An unknown floor string
/// ranks lowest, so it admits everything (fail-open — never silently drop).
pub fn severity_at_least(severity: &str, min_severity: Option<&str>) -> bool {
    min_severity.is_none_or(|floor| severity_rank(severity) >= severity_rank(floor))
}

/// Authentication configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
///
/// Also backfills `action_secret` (the push action-token HMAC signing key)
/// independently of the VAPID key pair, so an existing config that already has
/// VAPID keys — but predates the push action-token feature — still gets a
/// secret generated on the next startup. Returns `true` if *either* value was
/// generated.
pub fn ensure_vapid_keys(config: &mut Config) -> bool {
    let keys_generated = if config.notifications.vapid.private_key.is_empty()
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
    };

    let secret_generated = if config.notifications.vapid.action_secret.is_empty() {
        let secret_bytes: [u8; 32] = rand::random();
        config.notifications.vapid.action_secret =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret_bytes);
        true
    } else {
        false
    };

    keys_generated || secret_generated
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(default)]
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
    /// Burn-velocity governor: alert when a session's lifetime-average cost rate
    /// (USD/hour) exceeds this ceiling. `None` disables the cost-rate check.
    #[serde(default)]
    pub burn_ceiling_usd_per_hour: Option<f64>,
    /// Burn-velocity governor: alert when a session's lifetime-average token rate
    /// (tokens/hour) exceeds this ceiling. `None` disables the token-rate check.
    /// Covers agents with no cost signal (e.g. Codex).
    #[serde(default)]
    pub burn_ceiling_tokens_per_hour: Option<u64>,
    /// What to do when a session crosses a burn ceiling: `"alert"` (default,
    /// emit a `usage_alert.burn_ceiling` event only) or `"stop"` (also stop the
    /// session via the intervention path).
    #[serde(default = "default_burn_action")]
    pub burn_action: String,
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
        if self.burn_action != "alert" && self.burn_action != "stop" {
            anyhow::bail!(
                "watchdog.burn_action must be \"alert\" or \"stop\", got \"{}\"",
                self.burn_action
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
            burn_ceiling_usd_per_hour: None,
            burn_ceiling_tokens_per_hour: None,
            burn_action: default_burn_action(),
        }
    }
}

fn default_burn_action() -> String {
    String::from("alert")
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
#[serde(deny_unknown_fields)]
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
    /// Default command used when spawning a session without an explicit command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_command: Option<String>,
    /// Number of days to retain log files. Defaults to 7.
    #[serde(default = "default_log_retain_days")]
    pub log_retain_days: u32,
    /// Capture each session's full terminal output to `{data_dir}/logs/{id}.log`
    /// via `tmux pipe-pane`. Off by default: the capture is unbounded and writes
    /// every byte an agent prints, which fills the disk on long/chatty sessions.
    /// Enable only for debugging — the watchdog uses tmux scrollback for the live
    /// tail, and the last output snapshot is persisted in the database regardless.
    #[serde(default = "default_capture_session_output")]
    pub capture_session_output: bool,
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
            capture_session_output: default_capture_session_output(),
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

const fn default_capture_session_output() -> bool {
    false
}

fn default_data_dir() -> String {
    let fallback = String::from("~/.pulpo");
    dirs::home_dir().map_or(fallback, |h| {
        h.join(".pulpo").to_string_lossy().into_owned()
    })
}

impl Config {
    /// Build the usage rate overrides from `[rates.<model>]` config entries.
    pub fn rate_overrides(&self) -> crate::usage::RateOverrides {
        crate::usage::RateOverrides::new(self.rates.iter().map(|(model, r)| {
            (
                model.clone(),
                crate::usage::ModelRates {
                    input: r.input,
                    output: r.output,
                    cache_read: r.cache_read,
                    cache_write_5m: r.cache_write_5m,
                    cache_write_1h: r.cache_write_1h,
                },
            )
        }))
    }

    pub fn data_dir(&self) -> String {
        shellexpand::tilde(&self.node.data_dir).into_owned()
    }

    /// All configured webhook endpoints: the canonical top-level `[[webhooks]]`
    /// list unioned with the deprecated `[notifications.webhooks]` form.
    ///
    /// Top-level endpoints come first; legacy ones follow. Names are not
    /// deduplicated here — endpoint names are expected to be unique across both
    /// locations (the outbox resolves rows back to endpoints by name).
    pub fn webhook_endpoints(&self) -> Vec<WebhookEndpointConfig> {
        self.webhooks
            .iter()
            .chain(self.notifications.webhooks.iter())
            .cloned()
            .collect()
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
                bind: BindMode::default(),
                tag: None,
                discovery_interval_secs: default_discovery_interval_secs(),
                default_command: None,
                log_retain_days: default_log_retain_days(),
                capture_session_output: default_capture_session_output(),
            },
            ..Default::default()
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
    #[allow(clippy::float_cmp)]
    fn test_config_parses_rates_section_with_cache_defaults() {
        let toml_str = r#"
[node]
name = "test"

[rates."claude-opus-4-9"]
input = 5.0
output = 25.0
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let r = &config.rates["claude-opus-4-9"];
        assert_eq!(r.input, 5.0);
        assert_eq!(r.output, 25.0);
        // Omitted cache fields default to 0.0.
        assert_eq!(r.cache_read, 0.0);
        assert_eq!(r.cache_write_5m, 0.0);
        assert_eq!(r.cache_write_1h, 0.0);

        // The override is usable and prices a model the built-in table doesn't know.
        let overrides = config.rate_overrides();
        assert_eq!(
            crate::usage::resolve_rates("claude-opus-4-9", &overrides)
                .unwrap()
                .input,
            5.0
        );
    }

    #[test]
    fn test_rate_overrides_empty_when_unconfigured() {
        let config: Config = toml::from_str("[node]\nname = \"test\"\n").unwrap();
        assert!(config.rate_overrides().is_empty());
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
    fn test_load_rejects_unknown_top_level_section() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test-node"

[sandbox]
enabled = true
"#
        )
        .unwrap();

        let err = format!("{:#}", load(tmpfile.path().to_str().unwrap()).unwrap_err());
        assert!(err.contains("Failed to parse config"));
        assert!(err.contains("sandbox"));
    }

    #[test]
    fn test_load_rejects_deprecated_watchdog_ready_ttl_alias() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test-node"

[watchdog]
finished_ttl_secs = 60
"#
        )
        .unwrap();

        let err = format!("{:#}", load(tmpfile.path().to_str().unwrap()).unwrap_err());
        assert!(err.contains("Failed to parse config"));
        assert!(err.contains("finished_ttl_secs"));
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
            ..Default::default()
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
            ..Default::default()
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
    fn test_load_config_without_peers_defaults_empty() {
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
            peers,
            ..Default::default()
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
            ..Default::default()
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
            peers,
            ..Default::default()
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
            ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        let generated = ensure_auth_token(&mut config);
        assert!(!generated);
        assert_eq!(config.auth.token, "existing-token");
    }

    #[test]
    fn test_load_config_without_auth_defaults_empty() {
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
            ..Default::default()
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
            peers,
            ..Default::default()
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
    fn test_load_config_without_watchdog_defaults() {
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
                burn_ceiling_usd_per_hour: None,
                burn_ceiling_tokens_per_hour: None,
                burn_action: "alert".into(),
            },
            ..Default::default()
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
    fn test_watchdog_burn_defaults() {
        let wd = WatchdogConfig::default();
        assert_eq!(wd.burn_ceiling_usd_per_hour, None);
        assert_eq!(wd.burn_ceiling_tokens_per_hour, None);
        assert_eq!(wd.burn_action, "alert");
    }

    #[test]
    fn test_watchdog_validate_burn_action_alert() {
        let wd = WatchdogConfig {
            burn_action: "alert".into(),
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_watchdog_validate_burn_action_stop() {
        let wd = WatchdogConfig {
            burn_action: "stop".into(),
            ..WatchdogConfig::default()
        };
        assert!(wd.validate().is_ok());
    }

    #[test]
    fn test_watchdog_validate_burn_action_invalid() {
        let wd = WatchdogConfig {
            burn_action: "pause".into(),
            ..WatchdogConfig::default()
        };
        let err = wd.validate().unwrap_err();
        assert!(err.to_string().contains("burn_action"));
    }

    #[test]
    fn test_load_config_with_burn_ceilings() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "burn-cfg"

[watchdog]
burn_ceiling_usd_per_hour = 5.0
burn_ceiling_tokens_per_hour = 1000000
burn_action = "stop"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.watchdog.burn_ceiling_usd_per_hour, Some(5.0));
        assert_eq!(
            config.watchdog.burn_ceiling_tokens_per_hour,
            Some(1_000_000)
        );
        assert_eq!(config.watchdog.burn_action, "stop");
    }

    #[test]
    fn test_load_config_rejects_invalid_burn_action() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "bad-burn"

[watchdog]
burn_action = "explode"
"#
        )
        .unwrap();

        let result = load(tmpfile.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("burn_action"));
    }

    #[test]
    fn test_save_and_load_roundtrip_with_burn_ceilings() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("burn-rt.toml");
        let config = Config {
            node: NodeConfig {
                name: "burn-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            watchdog: WatchdogConfig {
                burn_ceiling_usd_per_hour: Some(2.5),
                burn_ceiling_tokens_per_hour: Some(500_000),
                burn_action: "stop".into(),
                ..WatchdogConfig::default()
            },
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.watchdog.burn_ceiling_usd_per_hour, Some(2.5));
        assert_eq!(loaded.watchdog.burn_ceiling_tokens_per_hour, Some(500_000));
        assert_eq!(loaded.watchdog.burn_action, "stop");
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
            watchdog: WatchdogConfig {
                idle_timeout_secs: 120,
                idle_action: "kill".into(),
                ..WatchdogConfig::default()
            },
            ..Default::default()
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
    fn test_load_config_with_legacy_inks_section() {
        // Configs written before the ink removal still load; the section is
        // tolerated and ignored.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[inks.reviewer]
command = "claude -p 'Custom review'"
description = "Code review specialist"

[inks.coder]
command = "codex -p 'Do it'"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert!(config.inks.is_some(), "legacy [inks] section is parsed");
    }

    #[test]
    fn test_save_drops_legacy_inks_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[inks.reviewer]
command = "claude -p 'Custom review'"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        save(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("[inks"),
            "retired [inks] section is dropped on save: {content}"
        );
        let reloaded = load(path.to_str().unwrap()).unwrap();
        assert!(reloaded.inks.is_none());
    }

    #[test]
    fn test_load_config_without_inks_section() {
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
        assert!(config.inks.is_none());
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
        assert!(config.notifications.webhooks.is_empty());
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
            notifications: NotificationsConfig {
                webhooks: vec![WebhookEndpointConfig {
                    name: "ci".into(),
                    url: "https://example.com/api/hooks/789/xyz".into(),
                    events: vec!["killed".into()],
                    min_severity: None,
                    secret: None,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.notifications.webhooks.len(), 1);
        assert_eq!(
            loaded.notifications.webhooks[0].url,
            "https://example.com/api/hooks/789/xyz"
        );
        assert_eq!(loaded.notifications.webhooks[0].events, vec!["killed"]);
    }

    #[test]
    fn test_notifications_config_default() {
        let config = NotificationsConfig::default();
        assert!(config.discord.is_none());
        assert!(config.webhooks.is_empty());
    }

    #[test]
    fn test_notifications_config_debug_clone() {
        let config = NotificationsConfig {
            webhooks: vec![WebhookEndpointConfig {
                name: "hook".into(),
                url: "url".into(),
                events: vec![],
                min_severity: None,
                secret: None,
            }],
            ..Default::default()
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
            min_severity: None,
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
            min_severity: None,
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
            notifications: NotificationsConfig {
                webhooks: vec![WebhookEndpointConfig {
                    name: "test-hook".into(),
                    url: "https://example.com/hook".into(),
                    events: vec!["killed".into()],
                    min_severity: None,
                    secret: Some("key".into()),
                }],
                ..Default::default()
            },
            ..Default::default()
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
            ..Default::default()
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
            capture_session_output: false,
        };
        let toml_str = toml::to_string(&config).unwrap();
        // tag should be skipped (None + skip_serializing_if)
        assert!(!toml_str.contains("tag"));
    }

    // -- VAPID key generation tests --

    #[test]
    fn test_vapid_config_default() {
        let vapid = VapidConfig::default();
        assert!(vapid.private_key.is_empty());
        assert!(vapid.public_key.is_empty());
        assert!(vapid.action_secret.is_empty());
    }

    #[test]
    fn test_vapid_config_debug_clone() {
        let vapid = VapidConfig {
            private_key: "priv".into(),
            public_key: "pub".into(),
            action_secret: "secret".into(),
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
            ..Default::default()
        };
        assert!(config.notifications.vapid.private_key.is_empty());
        assert!(config.notifications.vapid.public_key.is_empty());

        assert!(config.notifications.vapid.action_secret.is_empty());

        let generated = ensure_vapid_keys(&mut config);
        assert!(generated);
        assert!(!config.notifications.vapid.private_key.is_empty());
        assert!(!config.notifications.vapid.public_key.is_empty());
        assert!(!config.notifications.vapid.action_secret.is_empty());
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
            ..Default::default()
        };
        ensure_vapid_keys(&mut config);

        // Private key: 32 bytes → 43 chars base64url (no padding)
        assert_eq!(config.notifications.vapid.private_key.len(), 43);
        // Public key: 65 bytes → 87 chars base64url (no padding)
        assert_eq!(config.notifications.vapid.public_key.len(), 87);
        // Action secret: 32 bytes → 43 chars base64url (no padding)
        assert_eq!(config.notifications.vapid.action_secret.len(), 43);
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
            ..Default::default()
        };
        ensure_vapid_keys(&mut config);

        for key in [
            &config.notifications.vapid.private_key,
            &config.notifications.vapid.public_key,
            &config.notifications.vapid.action_secret,
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
            notifications: NotificationsConfig {
                vapid: VapidConfig {
                    private_key: "existing-private".into(),
                    public_key: "existing-public".into(),
                    action_secret: "existing-secret".into(),
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let generated = ensure_vapid_keys(&mut config);
        assert!(!generated);
        assert_eq!(config.notifications.vapid.private_key, "existing-private");
        assert_eq!(config.notifications.vapid.public_key, "existing-public");
        assert_eq!(config.notifications.vapid.action_secret, "existing-secret");
    }

    #[test]
    fn test_ensure_vapid_keys_backfills_action_secret_only() {
        // A config saved before the push action-token feature existed: VAPID keys
        // are already present, but action_secret predates the field and is empty.
        // The keys must survive untouched while the secret gets backfilled.
        let mut config = Config {
            notifications: NotificationsConfig {
                vapid: VapidConfig {
                    private_key: "existing-private".into(),
                    public_key: "existing-public".into(),
                    action_secret: String::new(),
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let generated = ensure_vapid_keys(&mut config);
        assert!(generated);
        assert_eq!(config.notifications.vapid.private_key, "existing-private");
        assert_eq!(config.notifications.vapid.public_key, "existing-public");
        assert!(!config.notifications.vapid.action_secret.is_empty());
    }

    #[test]
    fn test_ensure_vapid_keys_uniqueness() {
        let mut config1 = Config {
            node: NodeConfig::default(),
            ..Default::default()
        };
        let mut config2 = config1.clone();
        ensure_vapid_keys(&mut config1);
        ensure_vapid_keys(&mut config2);
        assert_ne!(
            config1.notifications.vapid.action_secret,
            config2.notifications.vapid.action_secret
        );
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
            ..Default::default()
        };
        ensure_vapid_keys(&mut config);
        let private_key = config.notifications.vapid.private_key.clone();
        let public_key = config.notifications.vapid.public_key.clone();
        let action_secret = config.notifications.vapid.action_secret.clone();

        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.notifications.vapid.private_key, private_key);
        assert_eq!(loaded.notifications.vapid.public_key, public_key);
        assert_eq!(loaded.notifications.vapid.action_secret, action_secret);
    }

    #[test]
    fn test_notifications_config_default_has_empty_vapid() {
        let config = NotificationsConfig::default();
        assert!(config.vapid.private_key.is_empty());
        assert!(config.vapid.public_key.is_empty());
        assert!(config.vapid.action_secret.is_empty());
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
            ..Default::default()
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
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("default_command"),
            "None should be omitted from serialized config"
        );
    }

    #[test]
    fn test_load_config_with_legacy_controller_section() {
        // Configs written before controller-mode removal still load; the section
        // is tolerated and ignored.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[controller]
enabled = true
stale_timeout_secs = 300
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert!(
            config.controller.is_some(),
            "legacy [controller] section is parsed"
        );
    }

    #[test]
    fn test_save_drops_legacy_controller_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
port = 7433

[controller]
address = "http://controller:7433"
token = "tok"
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        save(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("[controller]"),
            "retired [controller] section is dropped on save"
        );
    }

    #[test]
    fn test_load_config_with_legacy_docker_section() {
        // Configs written before the docker runtime removal still load.
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
volumes = ["~/.ssh:/root/.ssh:ro"]
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert!(config.docker.is_some(), "legacy [docker] section is parsed");
    }

    #[test]
    fn test_save_drops_legacy_docker_section() {
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
        save(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("[docker]"),
            "retired [docker] section is dropped on save: {content}"
        );
        let reloaded = load(path.to_str().unwrap()).unwrap();
        assert!(reloaded.docker.is_none());
    }

    #[test]
    fn test_load_config_without_docker_section() {
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
        assert!(config.docker.is_none());
    }

    // -- Controller mode config tests --

    #[test]
    fn test_load_tolerates_retired_discord_section() {
        // The Discord webhook notifier was removed, but pre-removal configs
        // may still carry a `[notifications.discord]` section. With
        // `deny_unknown_fields` on `NotificationsConfig`, that section must be
        // tolerated (captured into the ignored `discord` field) rather than
        // rejected at boot.
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "legacy"
port = 7433

[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/123/abc"
events = ["ready", "killed"]
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        // The section is captured but ignored.
        assert!(config.notifications.discord.is_some());
        assert!(config.notifications.webhooks.is_empty());
    }

    #[test]
    fn test_retired_discord_dropped_on_save() {
        // A captured legacy discord section must not be re-serialized on save.
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("discord-drop.toml");
        let config = Config {
            node: NodeConfig {
                name: "drop".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            notifications: NotificationsConfig {
                discord: Some(toml::Value::String("legacy".into())),
                ..NotificationsConfig::default()
            },
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("discord"),
            "retired discord field must not be serialized: {content}"
        );
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert!(loaded.notifications.discord.is_none());
    }

    // -- MetricsConfig tests --

    #[test]
    fn test_metrics_config_default_disabled() {
        assert!(!MetricsConfig::default().enabled);
    }

    #[test]
    fn test_metrics_config_clone_debug() {
        let mc = MetricsConfig { enabled: true };
        #[allow(clippy::redundant_clone)]
        let cloned = mc.clone();
        assert!(cloned.enabled);
        assert!(format!("{mc:?}").contains("MetricsConfig"));
    }

    #[test]
    fn test_load_config_without_metrics_section_defaults_off() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "no-metrics"
port = 7433
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(!config.metrics.enabled);
    }

    #[test]
    fn test_load_config_with_metrics_enabled() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "with-metrics"

[metrics]
enabled = true
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(config.metrics.enabled);
    }

    #[test]
    fn test_load_config_rejects_unknown_metrics_field() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "bad-metrics"

[metrics]
enabled = true
bogus = 1
"#
        )
        .unwrap();

        let result = load(tmpfile.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("bogus"));
    }

    #[test]
    fn test_save_and_load_roundtrip_with_metrics() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("metrics-rt.toml");
        let config = Config {
            node: NodeConfig {
                name: "metrics-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            metrics: MetricsConfig { enabled: true },
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert!(loaded.metrics.enabled);
    }

    // --- glob_match ---

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("lifecycle.idle", "lifecycle.idle"));
        assert!(!glob_match("lifecycle.idle", "lifecycle.active"));
        assert!(!glob_match("lifecycle.idle", "lifecycle"));
    }

    #[test]
    fn test_glob_match_prefix_glob() {
        assert!(glob_match("lifecycle.*", "lifecycle.idle"));
        assert!(glob_match("lifecycle.*", "lifecycle.active"));
        assert!(!glob_match("lifecycle.*", "usage_alert.budget_threshold"));
        // Prefix glob requires the dot separator, not a mere prefix string.
        assert!(!glob_match("life.*", "lifecycle.idle"));
    }

    #[test]
    fn test_glob_match_bare_type() {
        assert!(glob_match("lifecycle", "lifecycle.idle"));
        assert!(glob_match("usage_alert", "usage_alert.rate_limit"));
        assert!(!glob_match("lifecycle", "usage_alert.rate_limit"));
        // A bare type must not match a different type sharing a prefix.
        assert!(!glob_match("life", "lifecycle.idle"));
        // A bare type also matches the bare key with no subtype (exact match,
        // defensive — real event keys always carry a subtype).
        assert!(glob_match("lifecycle", "lifecycle"));
    }

    #[test]
    fn test_glob_match_star_matches_everything() {
        assert!(glob_match("*", "lifecycle.idle"));
        assert!(glob_match("*", "fleet.node_down"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn test_glob_match_no_match() {
        assert!(!glob_match("lifecycle.idle", "fleet.node_down"));
        assert!(!glob_match("intervention.*", "lifecycle.idle"));
    }

    // --- severity_at_least ---

    #[test]
    fn test_severity_rank_order() {
        assert!(severity_rank("info") < severity_rank("warn"));
        assert!(severity_rank("warn") < severity_rank("critical"));
        // Unknown severities rank lowest.
        assert_eq!(severity_rank("bogus"), 0);
    }

    #[test]
    fn test_severity_at_least_no_floor_admits_all() {
        assert!(severity_at_least("info", None));
        assert!(severity_at_least("critical", None));
    }

    #[test]
    fn test_severity_at_least_floor_warn() {
        assert!(!severity_at_least("info", Some("warn")));
        assert!(severity_at_least("warn", Some("warn")));
        assert!(severity_at_least("critical", Some("warn")));
    }

    #[test]
    fn test_severity_at_least_floor_critical() {
        assert!(!severity_at_least("info", Some("critical")));
        assert!(!severity_at_least("warn", Some("critical")));
        assert!(severity_at_least("critical", Some("critical")));
    }

    #[test]
    fn test_severity_at_least_unknown_floor_admits_all() {
        // A floor we cannot rank must never silently drop events (fail-open).
        assert!(severity_at_least("info", Some("bogus")));
    }

    // --- top-level [[webhooks]] parsing + legacy union ---

    #[test]
    fn test_load_top_level_webhooks() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "wh"

[[webhooks]]
name = "ops"
url = "https://example.com/ops"
events = ["lifecycle.*", "usage_alert.*"]
min_severity = "warn"
secret = "s3cret"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.webhooks.len(), 1);
        let w = &config.webhooks[0];
        assert_eq!(w.name, "ops");
        assert_eq!(w.url, "https://example.com/ops");
        assert_eq!(w.events, vec!["lifecycle.*", "usage_alert.*"]);
        assert_eq!(w.min_severity.as_deref(), Some("warn"));
        assert_eq!(w.secret.as_deref(), Some("s3cret"));
        // Legacy nested list stays empty.
        assert!(config.notifications.webhooks.is_empty());
    }

    #[test]
    fn test_load_webhook_min_severity_optional() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "wh"

[[webhooks]]
name = "all"
url = "https://example.com/all"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.webhooks.len(), 1);
        assert!(config.webhooks[0].events.is_empty());
        assert!(config.webhooks[0].min_severity.is_none());
    }

    #[test]
    fn test_load_legacy_notifications_webhooks_still_parse() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "wh"

[[notifications.webhooks]]
name = "legacy"
url = "https://example.com/legacy"
events = ["lifecycle.stopped"]
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(config.webhooks.is_empty());
        assert_eq!(config.notifications.webhooks.len(), 1);
        assert_eq!(config.notifications.webhooks[0].name, "legacy");
    }

    #[test]
    fn test_webhook_endpoints_union_top_level_first() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "wh"

[[webhooks]]
name = "canonical"
url = "https://example.com/canonical"

[[notifications.webhooks]]
name = "legacy"
url = "https://example.com/legacy"
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        let endpoints = config.webhook_endpoints();
        assert_eq!(endpoints.len(), 2);
        assert_eq!(endpoints[0].name, "canonical");
        assert_eq!(endpoints[1].name, "legacy");
    }

    #[test]
    fn test_webhook_endpoints_empty_by_default() {
        let config = load("/nonexistent/wh/config.toml").unwrap();
        assert!(config.webhook_endpoints().is_empty());
    }

    #[test]
    fn test_load_rejects_unknown_webhook_field() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "wh"

[[webhooks]]
name = "bad"
url = "https://example.com"
bogus_field = true
"#
        )
        .unwrap();
        let err = format!("{:#}", load(tmpfile.path().to_str().unwrap()).unwrap_err());
        assert!(err.contains("bogus_field"));
    }

    #[test]
    fn test_save_and_load_roundtrip_with_top_level_webhooks() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("wh-rt.toml");
        let config = Config {
            node: NodeConfig {
                name: "wh-rt".into(),
                port: 7433,
                data_dir: "/tmp".into(),
                ..NodeConfig::default()
            },
            webhooks: vec![WebhookEndpointConfig {
                name: "ops".into(),
                url: "https://example.com/ops".into(),
                events: vec!["lifecycle.*".into()],
                min_severity: Some("warn".into()),
                secret: Some("k".into()),
            }],
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.webhooks.len(), 1);
        assert_eq!(loaded.webhooks[0].name, "ops");
        assert_eq!(loaded.webhooks[0].events, vec!["lifecycle.*"]);
        assert_eq!(loaded.webhooks[0].min_severity.as_deref(), Some("warn"));
    }
}
