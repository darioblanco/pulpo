pub mod api;
pub mod backend;
pub mod config;
pub mod discovery;
pub mod guard;
pub mod mcp;
pub mod notifications;
pub mod peers;
pub mod platform;
pub mod session;
pub mod store;
pub mod watchdog;

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use pulpo_common::event::PulpoEvent;
use tokio::sync::{broadcast, watch};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[cfg(not(coverage))]
use backend::tmux::TmuxBackend;
use session::manager::SessionManager;

/// No-op backend used only during coverage builds (where TmuxBackend doesn't impl Backend).
#[cfg(coverage)]
struct CoverageBackend;

#[cfg(coverage)]
impl backend::Backend for CoverageBackend {
    fn session_id(&self, name: &str) -> String {
        name.to_owned()
    }
    fn create_session(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn kill_session(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn is_alive(&self, _: &str) -> anyhow::Result<bool> {
        Ok(true)
    }
    fn capture_output(&self, _: &str, _: usize) -> anyhow::Result<String> {
        Ok(String::new())
    }
    fn send_input(&self, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn setup_logging(&self, _: &str, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Holds shutdown senders for all background loops.
///
/// Calling `shutdown()` signals all loops to exit gracefully. Also holds owned
/// resources (like mDNS registration) that should be dropped on shutdown.
pub struct ShutdownHandle {
    senders: Vec<watch::Sender<bool>>,
    /// mDNS registration kept alive until shutdown (behind cfg so coverage builds compile).
    #[cfg(not(coverage))]
    mdns_registration: Option<discovery::mdns::MdnsRegistration>,
    /// Whether `tailscale serve` was started and needs cleanup on shutdown.
    tailscale_serve_active: bool,
}

impl ShutdownHandle {
    const fn new() -> Self {
        Self {
            senders: Vec::new(),
            #[cfg(not(coverage))]
            mdns_registration: None,
            tailscale_serve_active: false,
        }
    }

    fn add_sender(&mut self, tx: watch::Sender<bool>) {
        self.senders.push(tx);
    }

    #[cfg(not(coverage))]
    fn set_mdns_registration(&mut self, reg: discovery::mdns::MdnsRegistration) {
        self.mdns_registration = Some(reg);
    }

    /// Signal all background loops to shut down and clean up resources.
    pub fn shutdown(&self) {
        for tx in &self.senders {
            let _ = tx.send(true);
        }
        if self.tailscale_serve_active {
            tailscale_serve_cleanup();
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "pulpod",
    about = "Pulpo daemon — agent session orchestrator",
    version = env!("PULPO_VERSION")
)]
pub struct Cli {
    /// Config file path
    #[arg(long, default_value = "~/.pulpo/config.toml")]
    pub config: String,

    /// Port to listen on (overrides config)
    #[arg(short, long)]
    pub port: Option<u16>,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(clap::Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// Start the MCP server over STDIO (for use by AI agents)
    Mcp,
}

/// Initialize tracing subscriber for logging.
pub fn init_tracing() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("pulpod=info".parse()?))
        .try_init()
        .ok();
    Ok(())
}

/// Build the application from config — returns the router, listener address, and shutdown handle.
#[allow(clippy::too_many_lines)]
pub async fn build_app(cli: &Cli) -> Result<(axum::Router, String, ShutdownHandle)> {
    let mut config = config::load(&cli.config)?;
    let port = cli.port.unwrap_or(config.node.port);

    // Resolve config path for saving later
    let expanded = shellexpand::tilde(&cli.config);
    let config_path = std::path::PathBuf::from(expanded.as_ref());

    // Auto-generate auth token on first run
    if config::ensure_auth_token(&mut config) {
        info!("Generated new auth token");
        config::save(&config, &config_path)?;
    }

    let store = store::Store::new(&config.data_dir()).await?;
    store.migrate().await?;

    #[cfg(not(coverage))]
    let backend: Arc<dyn backend::Backend> = Arc::new(TmuxBackend::new());

    #[cfg(not(coverage))]
    {
        let version = backend.check_version()?;
        info!("Using {version}");
    }

    #[cfg(coverage)]
    let backend: Arc<dyn backend::Backend> = Arc::new(CoverageBackend);
    #[cfg(not(coverage))]
    let watchdog_backend = backend.clone();
    #[cfg(not(coverage))]
    let watchdog_store = store.clone();

    let default_guard = config.guards.to_guard_config();
    let node_name = config.node.name.clone();
    let (event_tx, _) = broadcast::channel::<PulpoEvent>(256);

    let manager = SessionManager::new(backend, store, default_guard, config.inks.clone())
        .with_default_provider(config.node.default_provider.clone())
        .with_session_defaults(config.session_defaults.clone())
        .with_event_tx(event_tx.clone(), node_name.clone());

    let peer_registry = peers::PeerRegistry::new(&config.peers);

    let mut shutdown_handle = ShutdownHandle::new();

    #[cfg(not(coverage))]
    let watchdog_config_tx = {
        if config.watchdog.enabled {
            let reader = watchdog::memory::SystemMemoryReader;
            let wd_runtime = watchdog::WatchdogRuntimeConfig {
                threshold: config.watchdog.memory_threshold,
                interval: std::time::Duration::from_secs(config.watchdog.check_interval_secs),
                breach_count: config.watchdog.breach_count,
                idle: watchdog::IdleConfig {
                    enabled: config.watchdog.idle_timeout_secs > 0,
                    timeout_secs: config.watchdog.idle_timeout_secs,
                    action: if config.watchdog.idle_action == "kill" {
                        watchdog::IdleAction::Kill
                    } else {
                        watchdog::IdleAction::Alert
                    },
                },
                finished_ttl_secs: config.watchdog.finished_ttl_secs,
            };
            let (wd_config_tx, wd_config_rx) = watch::channel(wd_runtime.clone());
            let (wd_shutdown_tx, wd_shutdown_rx) = watch::channel(false);
            info!(
                threshold = wd_runtime.threshold,
                interval_secs = wd_runtime.interval.as_secs(),
                breach_count = wd_runtime.breach_count,
                "Starting memory watchdog"
            );
            let finished_ctx = watchdog::FinishedContext {
                event_tx: Some(event_tx.clone()),
                node_name,
            };
            tokio::spawn(watchdog::run_watchdog_loop(
                watchdog_backend,
                watchdog_store,
                Box::new(reader),
                wd_config_rx,
                wd_shutdown_rx,
                finished_ctx,
            ));
            shutdown_handle.add_sender(wd_shutdown_tx);
            Some(wd_config_tx)
        } else {
            None
        }
    };

    let bind_mode = config.node.bind;

    // Start peer discovery based on bind mode
    #[cfg(not(coverage))]
    match bind_mode {
        pulpo_common::auth::BindMode::Tailscale => {
            let ts_registry = peer_registry.clone();
            let own_name = config.node.name.clone();
            let ts_tag = config.node.tag.clone();
            let ts_interval = std::time::Duration::from_secs(config.node.discovery_interval_secs);
            let (ts_shutdown_tx, ts_shutdown_rx) = watch::channel(false);
            tokio::spawn(discovery::tailscale::run_tailscale_discovery(
                ts_registry,
                own_name,
                ts_tag,
                ts_interval,
                ts_shutdown_rx,
            ));
            shutdown_handle.add_sender(ts_shutdown_tx);
            info!("Tailscale discovery enabled");
        }
        pulpo_common::auth::BindMode::Public => {
            if let Some(seed_address) = config.node.seed.clone() {
                // Seed discovery (explicit seed peer)
                let seed_registry = peer_registry.clone();
                let own_name = config.node.name.clone();
                let seed_interval =
                    std::time::Duration::from_secs(config.node.discovery_interval_secs);
                let (seed_shutdown_tx, seed_shutdown_rx) = watch::channel(false);
                tokio::spawn(discovery::seed::run_seed_discovery(
                    seed_registry,
                    own_name,
                    port,
                    seed_address,
                    seed_interval,
                    seed_shutdown_rx,
                ));
                shutdown_handle.add_sender(seed_shutdown_tx);
                info!("Seed discovery enabled");
            } else {
                // mDNS discovery (default for public)
                let reg = discovery::ServiceRegistration {
                    node_name: config.node.name.clone(),
                    port,
                };
                match discovery::mdns::MdnsRegistration::register(&reg) {
                    Ok(registration) => {
                        shutdown_handle.set_mdns_registration(registration);
                    }
                    Err(e) => {
                        tracing::warn!("mDNS registration failed (discovery disabled): {e}");
                    }
                }

                let browser_registry = peer_registry.clone();
                let own_name = config.node.name.clone();
                let (browser_shutdown_tx, browser_shutdown_rx) = watch::channel(false);
                tokio::spawn(discovery::mdns::run_mdns_browser(
                    browser_registry,
                    own_name,
                    browser_shutdown_rx,
                ));
                shutdown_handle.add_sender(browser_shutdown_tx);
            }
        }
        // Local and Container: no discovery
        pulpo_common::auth::BindMode::Local | pulpo_common::auth::BindMode::Container => {}
    }

    // Start Discord notification loop if configured
    if let Some(discord_config) = config.notifications.discord.clone() {
        let notifier = notifications::discord::DiscordNotifier::new(discord_config);
        let discord_rx = event_tx.subscribe();
        let (discord_shutdown_tx, discord_shutdown_rx) = watch::channel(false);
        tokio::spawn(notifications::discord::run_notification_loop(
            notifier,
            discord_rx,
            discord_shutdown_rx,
        ));
        shutdown_handle.add_sender(discord_shutdown_tx);
        info!("Discord notifications enabled");
    }

    // Start generic webhook notification loops
    for webhook_config in &config.notifications.webhooks {
        let notifier = notifications::webhook::WebhookNotifier::new(webhook_config.clone());
        let webhook_rx = event_tx.subscribe();
        let (webhook_shutdown_tx, webhook_shutdown_rx) = watch::channel(false);
        let name = webhook_config.name.clone();
        tokio::spawn(notifications::webhook::run_notification_loop(
            notifier,
            webhook_rx,
            webhook_shutdown_rx,
        ));
        shutdown_handle.add_sender(webhook_shutdown_tx);
        info!(webhook = %name, "Webhook notifications enabled");
    }

    #[cfg(not(coverage))]
    let wd_tx = watchdog_config_tx;
    #[cfg(coverage)]
    let wd_tx: Option<tokio::sync::watch::Sender<watchdog::WatchdogRuntimeConfig>> = None;

    let state = api::AppState::with_watchdog_tx(
        config,
        config_path,
        manager,
        peer_registry,
        event_tx,
        wd_tx,
    );

    let app = api::router(state);

    let bind_ip: String = match bind_mode {
        pulpo_common::auth::BindMode::Local | pulpo_common::auth::BindMode::Tailscale => {
            "127.0.0.1".into()
        }
        pulpo_common::auth::BindMode::Public | pulpo_common::auth::BindMode::Container => {
            "0.0.0.0".into()
        }
    };

    // Set up tailscale serve for HTTPS access over tailnet
    if bind_mode == pulpo_common::auth::BindMode::Tailscale {
        match tailscale_serve_start(port) {
            Ok(()) => {
                shutdown_handle.tailscale_serve_active = true;
            }
            Err(e) => {
                tracing::warn!(
                    "Tailscale serve unavailable ({e}). \
                     Dashboard will only be accessible locally at http://localhost:{port}. \
                     Start Tailscale to enable HTTPS access over your tailnet."
                );
            }
        }
    }

    let addr = format!("{bind_ip}:{port}");
    info!("pulpod v{} starting", env!("CARGO_PKG_VERSION"));
    if bind_mode == pulpo_common::auth::BindMode::Tailscale
        && shutdown_handle.tailscale_serve_active
    {
        let ts_name = resolve_tailscale_name().unwrap_or_else(|_| "your-machine".into());
        info!("Dashboard: https://{ts_name}");
    } else {
        info!("Dashboard: http://localhost:{port}");
    }
    info!("Listening on {addr} (bind={bind_mode})");

    Ok((app, addr, shutdown_handle))
}

/// Start `tailscale serve` to proxy the local port over HTTPS on the tailnet.
///
/// Cleans up any stale serve rules first (e.g., from a previous crash), then
/// registers `https / http://127.0.0.1:{port}` so the dashboard is available at
/// `https://<machine-name>.<tailnet>.ts.net`.
#[cfg(not(coverage))]
fn tailscale_serve_start(port: u16) -> Result<()> {
    // Clean up stale rules from a previous crash
    let _ = std::process::Command::new("tailscale")
        .args(["serve", "--https=443", "off"])
        .output();

    let output = std::process::Command::new("tailscale")
        .args([
            "serve",
            "--bg",
            "--https=443",
            &format!("http://127.0.0.1:{port}"),
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run `tailscale serve`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tailscale serve failed: {stderr}");
    }
    info!("tailscale serve started (proxying port {port} over HTTPS)");
    Ok(())
}

/// Stub for coverage builds.
#[cfg(coverage)]
fn tailscale_serve_start(_port: u16) -> Result<()> {
    Ok(())
}

/// Clean up `tailscale serve` on shutdown, logging any errors.
#[cfg(not(coverage))]
fn tailscale_serve_cleanup() {
    if let Err(e) = tailscale_serve_stop() {
        tracing::warn!("Failed to stop tailscale serve: {e}");
    }
}

/// Stub for coverage builds.
#[cfg(coverage)]
fn tailscale_serve_cleanup() {}

/// Stop `tailscale serve` and remove the HTTPS proxy rule.
#[cfg(not(coverage))]
fn tailscale_serve_stop() -> Result<()> {
    let output = std::process::Command::new("tailscale")
        .args(["serve", "--https=443", "off"])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run `tailscale serve off`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tailscale serve off failed: {stderr}");
    }
    tracing::info!("tailscale serve stopped");
    Ok(())
}

/// Stub for coverage builds.
#[cfg(coverage)]
#[cfg_attr(coverage, allow(dead_code))]
fn tailscale_serve_stop() -> Result<()> {
    Ok(())
}

/// Resolve the Tailscale HTTPS hostname (e.g., `raven.tailnet-name.ts.net`).
#[cfg(not(coverage))]
fn resolve_tailscale_name() -> Result<String> {
    let output = std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run `tailscale status`: {e}"))?;
    if !output.status.success() {
        anyhow::bail!("tailscale status failed");
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let dns_name = json["Self"]["DNSName"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_owned();
    if dns_name.is_empty() {
        anyhow::bail!("Could not resolve Tailscale DNS name");
    }
    Ok(dns_name)
}

/// Stub for coverage builds.
#[cfg(coverage)]
fn resolve_tailscale_name() -> Result<String> {
    Ok("test-node.tailnet.ts.net".into())
}

/// Build the MCP server from config — same init as `build_app` but returns `PulpoMcp`
/// instead of a router. No HTTP server, no tracing to stdout (would corrupt STDIO protocol).
pub async fn build_mcp_server(cli: &Cli) -> Result<mcp::PulpoMcp> {
    let mut config = config::load(&cli.config)?;

    // Resolve config path for saving later
    let expanded = shellexpand::tilde(&cli.config);
    let config_path = std::path::PathBuf::from(expanded.as_ref());

    // Auto-generate auth token on first run
    if config::ensure_auth_token(&mut config) {
        config::save(&config, &config_path)?;
    }

    let store = store::Store::new(&config.data_dir()).await?;
    store.migrate().await?;

    #[cfg(not(coverage))]
    let backend: Arc<dyn backend::Backend> = Arc::new(backend::tmux::TmuxBackend::new());

    #[cfg(coverage)]
    let backend: Arc<dyn backend::Backend> = Arc::new(CoverageBackend);

    let default_guard = config.guards.to_guard_config();
    let manager =
        session::manager::SessionManager::new(backend, store, default_guard, config.inks.clone())
            .with_default_provider(config.node.default_provider.clone())
            .with_session_defaults(config.session_defaults.clone());
    let peer_registry = peers::PeerRegistry::new(&config.peers);

    Ok(mcp::PulpoMcp::new(manager, peer_registry, config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_handle_signals_loops() {
        let mut handle = ShutdownHandle::new();
        let (tx1, mut rx1) = watch::channel(false);
        let (tx2, mut rx2) = watch::channel(false);
        handle.add_sender(tx1);
        handle.add_sender(tx2);

        assert!(!*rx1.borrow());
        assert!(!*rx2.borrow());

        handle.shutdown();

        assert!(rx1.has_changed().unwrap());
        assert!(*rx1.borrow_and_update());
        assert!(rx2.has_changed().unwrap());
        assert!(*rx2.borrow_and_update());
    }

    #[test]
    fn test_shutdown_handle_empty() {
        let handle = ShutdownHandle::new();
        // Should not panic with no senders
        handle.shutdown();
    }

    #[test]
    fn test_shutdown_handle_dropped_receiver() {
        let mut handle = ShutdownHandle::new();
        let (tx, rx) = watch::channel(false);
        handle.add_sender(tx);
        drop(rx);
        // Should not panic when receiver is already dropped
        handle.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_with_defaults() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");

        // Write a config that uses a temp data dir
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };

        let (app, addr, handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "127.0.0.1:0");
        // Verify app and handle can be used (doesn't panic)
        handle.shutdown();
        drop(app);

        // Token should have been auto-generated and saved
        let saved = config::load(config_path.to_str().unwrap()).unwrap();
        assert!(!saved.auth.token.is_empty());
        assert_eq!(saved.auth.token.len(), 43);
    }

    #[test]
    fn test_cli_version() {
        let result = Cli::try_parse_from(["pulpod", "--version"]);
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn test_cli_parse() {
        // Test default parsing
        let cli = Cli::try_parse_from(["pulpod"]).unwrap();
        assert_eq!(cli.config, "~/.pulpo/config.toml");
        assert!(cli.port.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_parse_with_args() {
        let cli =
            Cli::try_parse_from(["pulpod", "--config", "/custom/path", "--port", "8080"]).unwrap();
        assert_eq!(cli.config, "/custom/path");
        assert_eq!(cli.port, Some(8080));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_parse_mcp_subcommand() {
        let cli = Cli::try_parse_from(["pulpod", "mcp"]).unwrap();
        assert_eq!(cli.command, Some(CliCommand::Mcp));
    }

    #[test]
    fn test_cli_parse_mcp_with_config() {
        let cli = Cli::try_parse_from(["pulpod", "--config", "/custom/path", "mcp"]).unwrap();
        assert_eq!(cli.config, "/custom/path");
        assert_eq!(cli.command, Some(CliCommand::Mcp));
    }

    #[test]
    fn test_cli_command_debug() {
        let cmd = CliCommand::Mcp;
        let debug = format!("{cmd:?}");
        assert!(debug.contains("Mcp"));
    }

    #[test]
    fn test_cli_command_clone() {
        let cmd = CliCommand::Mcp;
        #[allow(clippy::clone_on_copy)]
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn test_init_tracing() {
        // Should not panic even if called multiple times (uses try_init)
        let result = init_tracing();
        assert!(result.is_ok());
    }

    #[cfg(coverage)]
    #[test]
    fn test_coverage_backend_methods() {
        use crate::backend::Backend;
        let b = CoverageBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_build_app_uses_config_port() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");

        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 9876
data_dir = "{}"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        // No port override — should use config's port
        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: None,
            command: None,
        };

        let (_app, addr, _handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "127.0.0.1:9876");
    }

    #[tokio::test]
    async fn test_build_mcp_server() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "mcp-test"
port = 0
data_dir = "{}"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: None,
            command: Some(CliCommand::Mcp),
        };

        let mcp = build_mcp_server(&cli).await.unwrap();
        let info = <mcp::PulpoMcp as rmcp::ServerHandler>::get_info(&mcp);
        assert_eq!(info.server_info.name, "pulpo");
    }

    #[tokio::test]
    async fn test_build_mcp_server_existing_token() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "mcp-token-test"
port = 0
data_dir = "{}"

[auth]
token = "already-existing-token"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: None,
            command: Some(CliCommand::Mcp),
        };

        let mcp = build_mcp_server(&cli).await.unwrap();
        let info = <mcp::PulpoMcp as rmcp::ServerHandler>::get_info(&mcp);
        assert_eq!(info.server_info.name, "pulpo");

        // Token should NOT have been overwritten
        let saved = config::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(saved.auth.token, "already-existing-token");
    }

    #[tokio::test]
    async fn test_build_app_with_discord_notifications() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"

[notifications.discord]
webhook_url = "https://discord.com/api/webhooks/123/abc"
events = ["finished", "killed"]
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };

        let (_app, addr, handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "127.0.0.1:0");
        // Shutdown should signal the discord notification loop too
        handle.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_bind_public() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"
bind = "public"

[auth]
token = "existing-token-value"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };
        let (_app, addr, _handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "0.0.0.0:0");

        // Existing token should be preserved
        let saved = config::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(saved.auth.token, "existing-token-value");
    }

    #[tokio::test]
    async fn test_build_app_bind_container() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"
bind = "container"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };
        let (_app, addr, _handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "0.0.0.0:0");
    }

    #[cfg(coverage)]
    #[tokio::test]
    async fn test_build_app_bind_tailscale() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"
bind = "tailscale"
tag = "pulpo"
discovery_interval_secs = 60
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };
        let (_app, addr, handle) = build_app(&cli).await.unwrap();
        // Tailscale bind uses 127.0.0.1 (tailscale serve proxies over HTTPS)
        assert_eq!(addr, "127.0.0.1:0");
        assert!(handle.tailscale_serve_active);
        handle.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_bind_public_with_seed() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"
bind = "public"
seed = "10.0.0.5:7433"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };
        let (_app, addr, handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "0.0.0.0:0");
        handle.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_bind_public_mdns() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"
bind = "public"
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };
        let (_app, addr, handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "0.0.0.0:0");
        handle.shutdown();
    }

    #[test]
    fn test_shutdown_handle_tailscale_serve_cleanup() {
        let mut handle = ShutdownHandle::new();
        assert!(!handle.tailscale_serve_active);
        handle.tailscale_serve_active = true;
        // Should not panic — coverage stub is a no-op
        handle.shutdown();
    }

    #[cfg(coverage)]
    #[test]
    fn test_tailscale_serve_stubs() {
        assert!(tailscale_serve_start(7433).is_ok());
        assert!(tailscale_serve_stop().is_ok());
        tailscale_serve_cleanup();
        assert_eq!(
            resolve_tailscale_name().unwrap(),
            "test-node.tailnet.ts.net"
        );
    }

    #[cfg(coverage)]
    #[test]
    fn test_coverage_backend_session_id() {
        use backend::Backend;
        let b = CoverageBackend;
        assert_eq!(b.session_id("my-session"), "my-session");
    }

    #[tokio::test]
    async fn test_build_app_with_webhooks() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "test"
port = 0
data_dir = "{}"

[[notifications.webhooks]]
name = "test-hook"
url = "http://127.0.0.1:1/hook"
events = ["killed"]
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };
        let (_app, addr, handle) = build_app(&cli).await.unwrap();
        assert_eq!(addr, "127.0.0.1:0");
        handle.shutdown();
    }
}
