use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use pulpo_common::auth::BindMode;
use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub node: NodeConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub watchdog: WatchdogConfig,
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
    /// Built-in cron scheduler tuning.
    #[serde(default)]
    pub scheduler: SchedulerConfig,
}

/// Built-in cron scheduler configuration (`[scheduler]`).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConfig {
    /// How often the scheduler checks for due schedules, in seconds. Production
    /// default is 60 (matching cron's own minute granularity); the end-to-end
    /// scenario suite (`crates/pulpo-e2e`) overrides this much lower so a schedule
    /// test doesn't have to wait a full minute for the first check after the
    /// schedule becomes due.
    #[serde(default = "default_scheduler_tick_secs")]
    pub tick_secs: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_secs: default_scheduler_tick_secs(),
        }
    }
}

const fn default_scheduler_tick_secs() -> u64 {
    60
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

/// Notification configuration (webhooks for status updates).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationsConfig {
    /// Generic webhook endpoints.
    ///
    /// **Deprecated location.** Prefer the canonical top-level `[[webhooks]]`
    /// table on [`Config`]. This nested form is still read for back-compat and
    /// unioned with the top-level list at startup, so configs written before the
    /// promotion keep working unchanged.
    #[serde(default)]
    pub webhooks: Vec<WebhookEndpointConfig>,
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
    /// Human-readable name for this endpoint. Must be unique across configured
    /// webhooks (used in logs to identify which endpoint a delivery targets).
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchdogConfig {
    #[serde(default = "default_watchdog_enabled")]
    pub enabled: bool,
    #[serde(default = "default_check_interval_secs")]
    pub check_interval_secs: u64,
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_idle_action")]
    pub idle_action: String,
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
        if self.check_interval_secs == 0 {
            anyhow::bail!("watchdog.check_interval_secs must be >= 1");
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
            check_interval_secs: default_check_interval_secs(),
            idle_timeout_secs: default_idle_timeout_secs(),
            idle_action: default_idle_action(),
            idle_threshold_secs: default_idle_threshold_secs(),
            waiting_patterns: Vec::new(),
        }
    }
}

const fn default_watchdog_enabled() -> bool {
    true
}

const fn default_check_interval_secs() -> u64 {
    10
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
    /// How the daemon binds to the network. Determines auth requirements and whether
    /// `tailscale serve` is used to expose the dashboard over the tailnet.
    #[serde(default)]
    pub bind: BindMode,
    /// Default command used when spawning a session without an explicit command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_command: Option<String>,
    /// Number of days to retain log files. Defaults to 7.
    #[serde(default = "default_log_retain_days")]
    pub log_retain_days: u32,
    /// Capture each session's full terminal output to `{data_dir}/logs/{id}.log`
    /// via `tmux pipe-pane`. On by default since ADR 0009: `wrap_command` no
    /// longer keeps a fallback shell open after the agent exits, so tmux tears a
    /// session's pane down the instant it ends — this pipe-pane log is the only
    /// place a session's very last lines of output survive that. Set to `false`
    /// to disable if the unbounded per-byte capture becomes a disk-usage concern
    /// on long/chatty sessions; the daemon still works fine without it, it just
    /// loses a `done` session's final output once its pane is gone.
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
            default_command: None,
            log_retain_days: default_log_retain_days(),
            capture_session_output: default_capture_session_output(),
        }
    }
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
    true
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

/// Table paths (dot-joined from the config root) whose *keys* are user-defined
/// names rather than a fixed struct's field set — currently only the
/// `[rates.<model>]` map. Every entry under such a path is accepted by name
/// (any model id goes), but the entry's own fields are still checked — against
/// the single representative sub-schema `schema_config` builds for it — so a
/// typo inside `[rates."claude-opus-4-9"]` is still caught.
fn is_freeform_table(path: &str) -> bool {
    path == "rates"
}

/// A `Config` with every field populated — including `Option`s, and one
/// representative entry in the free-form `rates` map and each `webhooks` list —
/// so that serializing it produces the complete tree of keys `Config` (and
/// everything nested inside it) can ever deserialize.
///
/// `load()` walks a parsed config file's [`toml::Value`] against this tree (see
/// [`strip_unknown_keys`]): any key that doesn't appear in it, at any nesting
/// level, is unknown and gets warned about and dropped before the real
/// `Config::deserialize` runs.
fn schema_config() -> Config {
    let mut rates = HashMap::new();
    rates.insert(
        String::from("__model__"),
        RateConfig {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write_5m: 0.0,
            cache_write_1h: 0.0,
        },
    );
    let webhook = WebhookEndpointConfig {
        name: String::new(),
        url: String::new(),
        events: Vec::new(),
        min_severity: Some(String::new()),
    };
    Config {
        node: NodeConfig {
            default_command: Some(String::new()),
            ..NodeConfig::default()
        },
        auth: AuthConfig::default(),
        watchdog: WatchdogConfig::default(),
        notifications: NotificationsConfig {
            webhooks: vec![webhook.clone()],
        },
        webhooks: vec![webhook],
        rates,
        scheduler: SchedulerConfig::default(),
    }
}

/// Recursively drop keys from `actual` that don't appear in `schema` at the same
/// position, recording each dropped key's dotted path (from the config root) in
/// `warnings`, and returning the cleaned value.
///
/// `path` is the dotted path to `actual`/`schema` so far (empty at the root). A
/// table under an [`is_freeform_table`] path accepts any key name — e.g.
/// `rates.claude-opus-4-9` — but still validates that entry's own fields against
/// `schema`'s single representative entry. An array (`[[webhooks]]`,
/// `[[notifications.webhooks]]`) validates every element against `schema`'s
/// single representative element, however many elements `actual` has.
///
/// Anything else — scalars, or a type mismatch between `actual` and `schema`
/// (a table where a string is expected, say) — is left untouched: that's a
/// genuine type error, not an unknown key, and surfaces later from `Config`'s
/// own deserialization with a precise message (e.g. `bind = "container"`).
fn strip_unknown_keys(
    actual: &toml::Value,
    schema: &toml::Value,
    path: &str,
    warnings: &mut Vec<String>,
) -> toml::Value {
    match (actual, schema) {
        (toml::Value::Table(actual_table), toml::Value::Table(schema_table)) => {
            let freeform = is_freeform_table(path);
            let freeform_schema = schema_table.values().next();
            let mut cleaned = toml::Table::new();
            for (key, value) in actual_table {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let sub_schema = if freeform {
                    freeform_schema
                } else {
                    schema_table.get(key)
                };
                match sub_schema {
                    Some(sub_schema) => {
                        cleaned.insert(
                            key.clone(),
                            strip_unknown_keys(value, sub_schema, &child_path, warnings),
                        );
                    }
                    None => warnings.push(child_path),
                }
            }
            toml::Value::Table(cleaned)
        }
        (toml::Value::Array(actual_items), toml::Value::Array(schema_items)) => {
            let element_schema = schema_items.first();
            toml::Value::Array(
                actual_items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| match element_schema {
                        Some(element_schema) => strip_unknown_keys(
                            item,
                            element_schema,
                            &format!("{path}[{index}]"),
                            warnings,
                        ),
                        None => item.clone(),
                    })
                    .collect(),
            )
        }
        _ => actual.clone(),
    }
}

/// Parse `content` as a config, dropping any unknown key — at any nesting level
/// — before deserializing into [`Config`]. Returns the dropped keys' dotted
/// paths (sorted) alongside the config.
///
/// This is the pure logic `load()` wraps: it stays a plain function of `&str`
/// so tests can assert on exactly which keys got flagged without going through
/// a temp file or a tracing subscriber.
fn parse_config(content: &str) -> Result<(Config, Vec<String>)> {
    let parsed: toml::Value = toml::from_str(content).context("Failed to parse config")?;
    let schema = toml::Value::try_from(schema_config())
        .expect("a fully-populated Config always serializes to a toml::Value");
    let mut warnings = Vec::new();
    let cleaned = strip_unknown_keys(&parsed, &schema, "", &mut warnings);
    warnings.sort();
    let config: Config = cleaned.try_into().context("Failed to parse config")?;
    Ok((config, warnings))
}

pub fn load(path: &str) -> Result<Config> {
    let expanded = shellexpand::tilde(path);
    let path = std::path::Path::new(expanded.as_ref());

    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let (config, warnings) = parse_config(&content)?;
        for unknown_key in &warnings {
            warn!("config: unknown key '{unknown_key}' ignored");
        }
        config.watchdog.validate()?;
        Ok(config)
    } else {
        // Return defaults if no config file exists
        Ok(Config::default())
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
        let config = Config {
            node: NodeConfig {
                name: "roundtrip".into(),
                port: 9000,
                data_dir: "/tmp/rt".into(),
                ..NodeConfig::default()
            },
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.name, "roundtrip");
        assert_eq!(loaded.node.port, 9000);
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
    fn test_watchdog_config_default() {
        let wc = WatchdogConfig::default();
        assert!(wc.enabled);
        assert_eq!(wc.check_interval_secs, 10);
        assert_eq!(wc.idle_timeout_secs, 600);
        assert_eq!(wc.idle_action, "alert");
    }

    #[test]
    fn test_watchdog_config_debug() {
        let wc = WatchdogConfig::default();
        let debug = format!("{wc:?}");
        assert!(debug.contains("enabled"));
        assert!(debug.contains("600"));
    }

    #[test]
    fn test_watchdog_config_clone() {
        let wc = WatchdogConfig::default();
        #[allow(clippy::redundant_clone)]
        let cloned = wc.clone();
        assert!(cloned.enabled);
        assert_eq!(cloned.check_interval_secs, 10);
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
        assert_eq!(config.watchdog.check_interval_secs, 10);
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
check_interval_secs = 5
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert!(!config.watchdog.enabled);
        assert_eq!(config.watchdog.check_interval_secs, 5);
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
                check_interval_secs: 30,
                idle_timeout_secs: 600,
                idle_action: "alert".into(),
                idle_threshold_secs: 60,
                waiting_patterns: Vec::new(),
            },
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert!(!loaded.watchdog.enabled);
        assert_eq!(loaded.watchdog.check_interval_secs, 30);
    }

    #[test]
    fn test_missing_config_has_default_watchdog() {
        let config = load("/nonexistent/watchdog/config.toml").unwrap();
        assert!(config.watchdog.enabled);
        assert_eq!(config.watchdog.check_interval_secs, 10);
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
        assert_eq!(config.watchdog.check_interval_secs, 10); // default
    }

    #[test]
    fn test_watchdog_validate_defaults_pass() {
        let wd = WatchdogConfig::default();
        assert!(wd.validate().is_ok());
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
                }],
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
            }],
        };
        let cloned = config.clone();
        assert_eq!(format!("{config:?}"), format!("{cloned:?}"));
    }

    #[test]
    fn test_webhook_endpoint_config_debug_clone() {
        let config = WebhookEndpointConfig {
            name: "hook".into(),
            url: "https://example.com".into(),
            events: vec!["killed".into()],
            min_severity: None,
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
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: WebhookEndpointConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.name, "ci");
        assert_eq!(parsed.url, "https://ci.example.com/hook");
        assert_eq!(parsed.events, vec!["ready"]);
    }

    #[test]
    fn test_webhook_endpoint_config_defaults() {
        let toml_str = r#"
name = "hook"
url = "https://example.com"
"#;
        let parsed: WebhookEndpointConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.events.is_empty());
        assert!(parsed.min_severity.is_none());
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
                }],
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
    }

    // -- Node bind config tests --

    #[test]
    fn test_node_config_default() {
        let node = NodeConfig::default();
        assert!(!node.name.is_empty());
        assert_eq!(node.port, 7433);
        assert_eq!(node.bind, pulpo_common::auth::BindMode::Local);
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
"#,
        )
        .unwrap();
        let config = load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.node.bind, pulpo_common::auth::BindMode::Tailscale);
    }

    /// `bind = "container"` (deploying pulpod itself inside Docker/Podman) was
    /// removed alongside `docker/` — a containerized pulpod can't see the agents'
    /// own session files that exact usage metering depends on. `bind` is a known
    /// key, so an invalid value like this is a proper enum-deserialization error
    /// (not an unknown key): loading such a config must fail loudly with a
    /// pointer to the remaining bind modes, rather than silently falling back to
    /// a default or being swallowed by the unknown-key warn-and-ignore path.
    #[test]
    fn test_load_config_rejects_bind_container() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[node]
name = "test"
bind = "container"
"#,
        )
        .unwrap();
        let err = load(path.to_str().unwrap()).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("container"), "message was: {message}");
        assert!(message.contains("local"), "message was: {message}");
        assert!(message.contains("tailscale"), "message was: {message}");
        assert!(message.contains("public"), "message was: {message}");
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
                ..NodeConfig::default()
            },
            ..Default::default()
        };
        save(&config, &path).unwrap();
        let loaded = load(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.node.bind, pulpo_common::auth::BindMode::Tailscale);
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

    // --- Generic unknown-key rule: warn and ignore, at any nesting level ---

    #[test]
    fn test_parse_config_warns_and_ignores_unknown_top_level_key() {
        let (config, warnings) = parse_config(
            r#"
[node]
name = "test-node"

[sandbox]
enabled = true
"#,
        )
        .unwrap();
        assert_eq!(warnings, vec!["sandbox".to_string()]);
        assert_eq!(config.node.name, "test-node");
    }

    #[test]
    fn test_parse_config_warns_and_ignores_unknown_nested_key() {
        let (config, warnings) = parse_config(
            r#"
[node]
name = "test-node"

[watchdog]
adopt_tmux = true
"#,
        )
        .unwrap();
        assert_eq!(warnings, vec!["watchdog.adopt_tmux".to_string()]);
        // The rest of the `[watchdog]` table around the unknown key still
        // parses normally, defaulted since nothing else was set.
        assert!(config.watchdog.enabled);
        assert_eq!(config.watchdog.check_interval_secs, 10);
    }

    #[test]
    fn test_load_from_file_warns_and_ignores_unknown_nested_key() {
        // Same as the parse_config-level test above, but through the public
        // `load()` file-path entry point end to end.
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmpfile,
            r#"
[node]
name = "test-node"

[watchdog]
adopt_tmux = true
"#
        )
        .unwrap();

        let config = load(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.node.name, "test-node");
        assert!(config.watchdog.enabled);
    }

    #[test]
    fn test_parse_config_warns_and_ignores_unknown_key_in_webhook_entry() {
        let (config, warnings) = parse_config(
            r#"
[node]
name = "wh"

[[webhooks]]
name = "ops"
url = "https://example.com/ops"
secret = "s3cret"
"#,
        )
        .unwrap();
        assert_eq!(warnings, vec!["webhooks[0].secret".to_string()]);
        assert_eq!(config.webhooks.len(), 1);
        assert_eq!(config.webhooks[0].name, "ops");
        assert_eq!(config.webhooks[0].url, "https://example.com/ops");
    }

    #[test]
    fn test_parse_config_warns_and_ignores_unknown_key_in_rate_entry() {
        let (config, warnings) = parse_config(
            r#"
[node]
name = "test"

[rates."claude-opus-4-9"]
input = 5.0
output = 25.0
markup_percent = 10
"#,
        )
        .unwrap();
        assert_eq!(
            warnings,
            vec!["rates.claude-opus-4-9.markup_percent".to_string()]
        );
        assert_eq!(config.rates["claude-opus-4-9"].input, 5.0);
        assert_eq!(config.rates["claude-opus-4-9"].output, 25.0);
    }

    #[test]
    fn test_parse_config_warns_on_typo_of_known_key_and_uses_default() {
        // `idle_actoin` is a typo of `idle_action` — not a special-cased alias,
        // just another unknown key: warned, dropped, and the real field keeps
        // its default rather than picking up the typo's value.
        let (config, warnings) = parse_config(
            r#"
[node]
name = "test"

[watchdog]
idle_actoin = "kill"
"#,
        )
        .unwrap();
        assert_eq!(warnings, vec!["watchdog.idle_actoin".to_string()]);
        assert_eq!(config.watchdog.idle_action, "alert");
    }

    #[test]
    fn test_parse_config_fully_valid_config_has_no_warnings() {
        let (config, warnings) = parse_config(
            r#"
[node]
name = "test-node"
port = 9999
data_dir = "/tmp/pulpo-test"
bind = "public"
default_command = "claude"
log_retain_days = 14
capture_session_output = true

[auth]
token = "tok"

[watchdog]
enabled = true
check_interval_secs = 10
idle_timeout_secs = 600
idle_action = "alert"
idle_threshold_secs = 60
waiting_patterns = ["custom>"]

[scheduler]
tick_secs = 30

[rates."claude-opus-4-9"]
input = 5.0
output = 25.0
cache_read = 0.5
cache_write_5m = 6.25
cache_write_1h = 10.0

[[webhooks]]
name = "ops"
url = "https://example.com/ops"
events = ["lifecycle.*"]
min_severity = "warn"

[[notifications.webhooks]]
name = "legacy"
url = "https://example.com/legacy"
"#,
        )
        .unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(config.node.name, "test-node");
        assert_eq!(config.webhooks.len(), 1);
        assert_eq!(config.notifications.webhooks.len(), 1);
        assert_eq!(config.rates["claude-opus-4-9"].output, 25.0);
    }

    #[test]
    fn test_parse_config_owner_shape_has_exactly_expected_warnings() {
        // Structure mirrors `examples/config/public-with-auth.toml` and
        // `examples/config/watchdog.toml`, plus five keys retired across
        // September 2026's cleanup — one per nesting shape the generic
        // unknown-key rule has to handle: a nested field under a known table
        // (`node.tag`, `watchdog.adopt_tmux`), a bare unknown top-level table
        // (`[metrics]`), an unknown top-level table with its own nested
        // content (`[plans.max]`), and an unknown field inside a
        // `[[webhooks]]` entry (`secret`).
        let (config, warnings) = parse_config(
            r#"
[node]
name = "my-server"
port = 7433
bind = "public"
tag = "pulpo"

[auth]
token = "replace-with-long-random-token"

[watchdog]
enabled = true
check_interval_secs = 10
idle_threshold_secs = 60
idle_timeout_secs = 600
idle_action = "alert"
adopt_tmux = true
waiting_patterns = ["custom-tool>"]

[metrics]
enabled = true

[plans.max]
weekly_token_allowance = 1000000

[[webhooks]]
name = "ops"
url = "https://example.com/hooks/pulpo"
secret = "s3cret"
"#,
        )
        .unwrap();

        assert_eq!(
            warnings,
            vec![
                "metrics".to_string(),
                "node.tag".to_string(),
                "plans".to_string(),
                "watchdog.adopt_tmux".to_string(),
                "webhooks[0].secret".to_string(),
            ]
        );
        // Every known field around the retired keys still loads correctly.
        assert_eq!(config.node.name, "my-server");
        assert_eq!(config.node.bind, pulpo_common::auth::BindMode::Public);
        assert_eq!(config.auth.token, "replace-with-long-random-token");
        assert_eq!(config.watchdog.check_interval_secs, 10);
        assert_eq!(config.watchdog.waiting_patterns, vec!["custom-tool>"]);
        assert_eq!(config.webhooks.len(), 1);
        assert_eq!(config.webhooks[0].url, "https://example.com/hooks/pulpo");
    }
}
