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
}

impl ShutdownHandle {
    const fn new() -> Self {
        Self {
            senders: Vec::new(),
            #[cfg(not(coverage))]
            mdns_registration: None,
        }
    }

    fn add_sender(&mut self, tx: watch::Sender<bool>) {
        self.senders.push(tx);
    }

    #[cfg(not(coverage))]
    fn set_mdns_registration(&mut self, reg: discovery::mdns::MdnsRegistration) {
        self.mdns_registration = Some(reg);
    }

    /// Signal all background loops to shut down.
    pub fn shutdown(&self) {
        for tx in &self.senders {
            let _ = tx.send(true);
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "pulpod",
    about = "Pulpo daemon — agent session orchestrator",
    version
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
    {
        let version = backend::tmux::check_tmux_version()?;
        info!("Using {version}");
    }

    #[cfg(not(coverage))]
    let backend: Arc<dyn backend::Backend> = Arc::new(TmuxBackend::new());

    #[cfg(coverage)]
    let backend: Arc<dyn backend::Backend> = Arc::new(CoverageBackend);
    #[cfg(not(coverage))]
    let watchdog_backend = backend.clone();
    #[cfg(not(coverage))]
    let watchdog_store = store.clone();

    let default_guard = config.guards.to_guard_config();
    let node_name = config.node.name.clone();
    let (event_tx, _) = broadcast::channel::<PulpoEvent>(256);
    let manager = SessionManager::new(backend, store, default_guard, config.personas.clone())
        .with_guardrail_defaults(
            config.guards.max_turns,
            config.guards.max_budget_usd,
            config.guards.output_format.clone(),
        )
        .with_event_tx(event_tx.clone(), node_name);

    let peer_registry = peers::PeerRegistry::new(&config.peers);

    let mut shutdown_handle = ShutdownHandle::new();

    #[cfg(not(coverage))]
    {
        if config.watchdog.enabled {
            let reader = watchdog::memory::SystemMemoryReader;
            let wd_threshold = config.watchdog.memory_threshold;
            let wd_interval = std::time::Duration::from_secs(config.watchdog.check_interval_secs);
            let wd_breach_count = config.watchdog.breach_count;
            let (wd_shutdown_tx, wd_shutdown_rx) = watch::channel(false);
            info!(
                threshold = wd_threshold,
                interval_secs = wd_interval.as_secs(),
                breach_count = wd_breach_count,
                "Starting memory watchdog"
            );
            let wd_idle = watchdog::IdleConfig {
                enabled: config.watchdog.idle_timeout_secs > 0,
                timeout_secs: config.watchdog.idle_timeout_secs,
                action: if config.watchdog.idle_action == "kill" {
                    watchdog::IdleAction::Kill
                } else {
                    watchdog::IdleAction::Alert
                },
            };
            tokio::spawn(watchdog::run_watchdog_loop(
                watchdog_backend,
                watchdog_store,
                Box::new(reader),
                wd_threshold,
                wd_interval,
                wd_breach_count,
                wd_idle,
                wd_shutdown_rx,
            ));
            shutdown_handle.add_sender(wd_shutdown_tx);
        }
    }

    let bind_mode = config.auth.bind;

    // Start mDNS registration + browsing when binding to public network
    #[cfg(not(coverage))]
    if bind_mode == pulpo_common::auth::BindMode::Public {
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

        // Start mDNS browsing for other pulpo daemons
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

    let state = api::AppState::with_event_tx(config, config_path, manager, peer_registry, event_tx);
    let app = api::router(state);

    let bind_ip = match bind_mode {
        pulpo_common::auth::BindMode::Local => "127.0.0.1",
        pulpo_common::auth::BindMode::Public | pulpo_common::auth::BindMode::Container => "0.0.0.0",
    };
    let addr = format!("{bind_ip}:{port}");
    info!("pulpod v{} starting", env!("CARGO_PKG_VERSION"));
    info!("Dashboard: http://localhost:{port}");
    info!("Listening on {addr} (bind={bind_mode})");

    Ok((app, addr, shutdown_handle))
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
    let manager = session::manager::SessionManager::new(
        backend,
        store,
        default_guard,
        config.personas.clone(),
    )
    .with_guardrail_defaults(
        config.guards.max_turns,
        config.guards.max_budget_usd,
        config.guards.output_format.clone(),
    );
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
events = ["completed", "dead"]
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

[auth]
token = "existing-token-value"
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

[auth]
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
}
