pub mod api;
pub mod auth_info;
pub mod backend;
pub mod config;
pub mod controller;
pub mod discovery;

pub mod mcp;
pub mod node;
pub mod notifications;
pub mod peers;
pub mod platform;
pub mod scheduler;
pub mod session;
pub mod store;
pub mod watchdog;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use pulpo_common::event::PulpoEvent;
use tokio::sync::{broadcast, watch};
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[cfg(all(not(coverage), not(target_os = "windows")))]
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
    fn list_sessions(&self) -> anyhow::Result<Vec<(String, String)>> {
        Ok(Vec::new())
    }
    fn pane_info(&self, _: &str) -> anyhow::Result<(String, String)> {
        Ok(("bash".into(), "/tmp".into()))
    }
}

/// Stub backend for platforms where tmux is not available (Windows).
/// Sessions require --runtime docker on these platforms.
#[cfg(target_os = "windows")]
struct WindowsStubBackend;

#[cfg(target_os = "windows")]
impl backend::Backend for WindowsStubBackend {
    fn create_session(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
        anyhow::bail!("tmux is not available on Windows — use --runtime docker for Docker sessions")
    }
    fn kill_session(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn is_alive(&self, _: &str) -> anyhow::Result<bool> {
        Ok(false)
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
/// Calling `shutdown()` signals all loops to exit gracefully.
pub struct ShutdownHandle {
    senders: Vec<watch::Sender<bool>>,
    /// Whether `tailscale serve` was started and needs cleanup on shutdown.
    tailscale_serve_active: bool,
}

impl ShutdownHandle {
    const fn new() -> Self {
        Self {
            senders: Vec::new(),
            tailscale_serve_active: false,
        }
    }

    fn add_sender(&mut self, tx: watch::Sender<bool>) {
        self.senders.push(tx);
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
///
/// When `log_dir` is `Some`, logs are written to hourly-rotated files under
/// `{log_dir}/logs/` using a non-blocking writer. The `retain_days` parameter
/// controls how many days of log files to keep (converted to `days * 24` hourly
/// files for `max_log_files`). If the log directory cannot be created, falls back
/// to console-only logging instead of failing.
///
/// The console layer is included only when stdout is a terminal (i.e., not when
/// running under systemd/launchd), to avoid double-logging to both journald and
/// the log file.
///
/// When `log_dir` is `None`, only console output is used (useful for tests).
///
/// Returns an optional guard that must be held for the lifetime of the program
/// to ensure buffered log writes are flushed.
pub fn init_tracing(
    log_dir: Option<&Path>,
    retain_days: u32,
) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use std::io::IsTerminal;
    use tracing_appender::rolling::{RollingFileAppender, Rotation};

    let env_filter = EnvFilter::from_default_env().add_directive("pulpod=info".parse()?);
    let is_tty = std::io::stdout().is_terminal();

    if let Some(dir) = log_dir {
        let log_path = dir.join("logs");
        match std::fs::create_dir_all(&log_path) {
            Ok(()) => {
                let max_files = retain_days.max(1) as usize * 24;
                let file_appender = RollingFileAppender::builder()
                    .rotation(Rotation::HOURLY)
                    .filename_prefix("pulpod.log")
                    .max_log_files(max_files)
                    .build(&log_path)
                    .map_err(|e| anyhow::anyhow!("Failed to create log appender: {e}"))?;
                let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(non_blocking);

                // Include console layer only when running interactively (TTY).
                // Under systemd/launchd, stdout goes to journald/syslog already.
                let console_layer = is_tty.then(tracing_subscriber::fmt::layer);

                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(console_layer)
                    .with(file_layer)
                    .try_init()
                    .ok();

                return Ok(Some(guard));
            }
            Err(e) => {
                eprintln!(
                    "Warning: could not create log directory {}: {e}. Logging to console only.",
                    log_path.display()
                );
            }
        }
    }

    let console_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .try_init()
        .ok();

    Ok(None)
}

/// Upgrade name-based backend session IDs to tmux `$N` IDs for live sessions.
/// Best-effort: skips sessions whose tmux session is dead or already upgraded.
#[cfg(all(not(coverage), not(target_os = "windows")))]
async fn upgrade_backend_ids(manager: &SessionManager, store: &store::Store) {
    let upgrade_backend = manager.backend();
    let Ok(sessions) = store.list_sessions().await else {
        return;
    };
    for session in sessions {
        let is_live = matches!(
            session.status,
            pulpo_common::session::SessionStatus::Active
                | pulpo_common::session::SessionStatus::Idle
                | pulpo_common::session::SessionStatus::Ready
        );
        if !is_live {
            continue;
        }
        if session
            .backend_session_id
            .as_ref()
            .is_some_and(|id| id.starts_with('$') || id.starts_with("docker:"))
        {
            continue;
        }
        if let Ok(tmux_id) = upgrade_backend.query_backend_id(&session.name) {
            let _ = store
                .update_backend_session_id(&session.id.to_string(), &tmux_id)
                .await;
        }
    }
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
    let mut config_changed = config::ensure_auth_token(&mut config);
    if config_changed {
        info!("Generated new auth token");
    }

    // Auto-generate VAPID keys on first run
    if config::ensure_vapid_keys(&mut config) {
        info!("Generated new VAPID keys for Web Push");
        config_changed = true;
    }

    if config_changed {
        config::save(&config, &config_path)?;
    }

    let store = store::Store::new(&config.data_dir()).await?;
    store.migrate().await?;

    #[cfg(all(not(coverage), not(target_os = "windows")))]
    let backend: Arc<dyn backend::Backend> = Arc::new(TmuxBackend::new());

    #[cfg(all(not(coverage), not(target_os = "windows")))]
    {
        let version = backend.check_version()?;
        info!("Using {version}");
    }

    #[cfg(all(not(coverage), target_os = "windows"))]
    let backend: Arc<dyn backend::Backend> = Arc::new(WindowsStubBackend);

    #[cfg(coverage)]
    let backend: Arc<dyn backend::Backend> = Arc::new(CoverageBackend);
    #[cfg(not(coverage))]
    let watchdog_backend = backend.clone();
    #[cfg(not(coverage))]
    let watchdog_store = store.clone();

    let node_name = config.node.name.clone();
    let (event_tx, _) = broadcast::channel::<PulpoEvent>(256);

    let docker_backend: Option<Arc<dyn backend::Backend>> = if config.docker.image.is_empty() {
        None
    } else {
        #[cfg(not(coverage))]
        {
            Some(Arc::new(backend::docker::DockerBackend::new(
                &config.docker.image,
                config.docker.volumes.clone(),
            )))
        }
        #[cfg(coverage)]
        {
            None
        }
    };

    let mut manager = SessionManager::new(
        backend,
        store.clone(),
        config.inks.clone(),
        config.node.default_command.clone(),
    )
    .with_event_tx(event_tx.clone(), node_name.clone());
    if let Some(ref db) = docker_backend {
        manager = manager.with_docker_backend(db.clone());
    }

    // Auto-resume sessions that were active before a restart
    match manager.resume_lost_sessions().await {
        Ok(0) => {}
        Ok(n) => info!("Auto-resumed {n} session(s) from previous run"),
        Err(e) => tracing::warn!("Failed to auto-resume sessions: {e}"),
    }

    // Upgrade name-based backend_session_ids to tmux $N IDs (best-effort)
    #[cfg(all(not(coverage), not(target_os = "windows")))]
    upgrade_backend_ids(&manager, &store).await;

    let peer_registry = peers::PeerRegistry::new(&config.peers);

    let mut shutdown_handle = ShutdownHandle::new();

    // Start built-in scheduler
    #[cfg(not(coverage))]
    {
        let sched_manager = manager.clone();
        let sched_store = store.clone();
        let sched_role = config.role();
        let sched_local_node_name = node_name.clone();
        let sched_peer_registry = peer_registry.clone();
        let sched_event_tx = Some(event_tx.clone());
        let (sched_shutdown_tx, sched_shutdown_rx) = watch::channel(false);
        tokio::spawn(scheduler::run_scheduler_loop(
            sched_manager,
            sched_store,
            sched_role,
            sched_local_node_name,
            sched_peer_registry,
            sched_event_tx,
            sched_shutdown_rx,
        ));
        shutdown_handle.add_sender(sched_shutdown_tx);
        info!("Scheduler enabled");
    }

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
                    threshold_secs: config.watchdog.idle_threshold_secs,
                },
                ready_ttl_secs: config.watchdog.ready_ttl_secs,
                adopt_tmux: config.watchdog.adopt_tmux,
                extra_waiting_patterns: config.watchdog.waiting_patterns.clone(),
            };
            let (wd_config_tx, wd_config_rx) = watch::channel(wd_runtime.clone());
            let (wd_shutdown_tx, wd_shutdown_rx) = watch::channel(false);
            info!(
                threshold = wd_runtime.threshold,
                interval_secs = wd_runtime.interval.as_secs(),
                breach_count = wd_runtime.breach_count,
                "Starting memory watchdog"
            );
            let ready_ctx = watchdog::ReadyContext {
                event_tx: Some(event_tx.clone()),
                node_name,
            };
            tokio::spawn(watchdog::run_watchdog_loop(
                watchdog_backend,
                watchdog_store,
                Box::new(reader),
                wd_config_rx,
                wd_shutdown_rx,
                ready_ctx,
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
        // Public, Local, and Container: no automatic discovery.
        // Use manual [peers] config for multi-node in these modes.
        pulpo_common::auth::BindMode::Public
        | pulpo_common::auth::BindMode::Local
        | pulpo_common::auth::BindMode::Container => {}
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

    // Start Web Push notification loop (always enabled when VAPID keys are present)
    if !config.notifications.vapid.private_key.is_empty()
        && !config.notifications.vapid.public_key.is_empty()
    {
        let notifier = notifications::web_push::WebPushNotifier::new(
            store.clone(),
            config.notifications.vapid.private_key.clone(),
        );
        let push_rx = event_tx.subscribe();
        let (push_shutdown_tx, push_shutdown_rx) = watch::channel(false);
        tokio::spawn(notifications::web_push::run_notification_loop(
            notifier,
            push_rx,
            push_shutdown_rx,
        ));
        shutdown_handle.add_sender(push_shutdown_tx);
        info!("Web Push notifications enabled");
    }

    #[cfg(not(coverage))]
    let wd_tx = watchdog_config_tx;
    #[cfg(coverage)]
    let wd_tx: Option<tokio::sync::watch::Sender<watchdog::WatchdogRuntimeConfig>> = None;

    // Build AppState based on node role
    let role = config.role();
    let (session_index, command_queue) = match role {
        config::NodeRole::Controller => {
            let si = Arc::new(controller::SessionIndex::new());
            let cq = Arc::new(controller::CommandQueue::new());
            match store.list_master_session_index_entries().await {
                Ok(entries) => {
                    for entry in entries {
                        si.upsert(entry).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to hydrate controller session index from store: {e}");
                }
            }
            match store.list_master_workers().await {
                Ok(workers) => {
                    for (node_name, seen_at) in workers {
                        si.restore_worker(&node_name, seen_at).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to hydrate controller node heartbeats from store: {e}");
                }
            }
            info!("Controller mode enabled");
            info!(
                "Controller command queue is in-memory only; pending node commands do not survive controller restart"
            );
            #[cfg(not(coverage))]
            {
                let stale_si = si.clone();
                let stale_store = store.clone();
                let stale_timeout =
                    std::time::Duration::from_secs(config.controller.stale_timeout_secs);
                let stale_interval = std::time::Duration::from_secs(60);
                let stale_event_tx = event_tx.clone();
                let (stale_shutdown_tx, stale_shutdown_rx) = watch::channel(false);
                tokio::spawn(controller::run_stale_cleanup_loop(
                    stale_si,
                    stale_store,
                    stale_timeout,
                    stale_interval,
                    stale_event_tx,
                    stale_shutdown_rx,
                ));
                shutdown_handle.add_sender(stale_shutdown_tx);
                info!(
                    stale_timeout_secs = config.controller.stale_timeout_secs,
                    "Controller stale cleanup enabled"
                );
            }
            (Some(si), Some(cq))
        }
        config::NodeRole::Node | config::NodeRole::Standalone => (None, None),
    };

    let state = api::AppState::with_all(
        config.clone(),
        config_path,
        manager.clone(),
        peer_registry,
        event_tx.clone(),
        wd_tx,
        store.clone(),
        session_index,
        command_queue,
    );

    // Spawn node loops when running in Node mode
    #[cfg(not(coverage))]
    if role == config::NodeRole::Node {
        let controller_url = config.controller.address.clone().unwrap_or_default();
        let controller_token = config
            .controller
            .token
            .clone()
            .expect("node mode requires controller.token");
        let node_name = config.node.name.clone();

        // Event push loop
        let push_rx = event_tx.subscribe();
        let (push_shutdown_tx, push_shutdown_rx) = watch::channel(false);
        tokio::spawn(node::event_push::run_event_push_loop(
            controller_url.clone(),
            controller_token.clone(),
            node_name.clone(),
            push_rx,
            push_shutdown_rx,
        ));
        shutdown_handle.add_sender(push_shutdown_tx);

        // Command poll loop
        let (poll_shutdown_tx, poll_shutdown_rx) = watch::channel(false);
        tokio::spawn(node::command_poll::run_command_poll_loop(
            controller_url,
            controller_token,
            node_name,
            manager,
            poll_shutdown_rx,
        ));
        shutdown_handle.add_sender(poll_shutdown_tx);
        info!("Node mode enabled: pushing events and polling controller commands");
    }

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

    #[cfg(all(not(coverage), not(target_os = "windows")))]
    let backend: Arc<dyn backend::Backend> = Arc::new(backend::tmux::TmuxBackend::new());

    #[cfg(all(not(coverage), target_os = "windows"))]
    let backend: Arc<dyn backend::Backend> = Arc::new(WindowsStubBackend);

    #[cfg(coverage)]
    let backend: Arc<dyn backend::Backend> = Arc::new(CoverageBackend);

    let manager = session::manager::SessionManager::new(
        backend,
        store.clone(),
        config.inks.clone(),
        config.node.default_command.clone(),
    );
    let peer_registry = peers::PeerRegistry::new(&config.peers);

    Ok(mcp::PulpoMcp::new(manager, peer_registry, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use axum_test::TestServer;
    use pulpo_common::api::{
        EnrollNodeRequest, EnrollNodeResponse, EventPushRequest, FleetSessionsResponse,
        NodeCommand, NodeCommandsResponse, SessionIndexEntry,
    };
    use pulpo_common::event::{PulpoEvent, SessionEvent};

    #[allow(clippy::future_not_send)]
    async fn enroll_node(server: &TestServer, node_name: &str) -> String {
        let resp = server
            .post("/api/v1/controller/nodes")
            .json(&EnrollNodeRequest {
                node_name: node_name.into(),
            })
            .await;
        resp.assert_status(axum::http::StatusCode::CREATED);
        let body: EnrollNodeResponse = resp.json();
        body.token
    }

    #[allow(clippy::future_not_send)]
    async fn push_session_event(
        server: &TestServer,
        token: &str,
        session_id: &str,
        session_name: &str,
        status: &str,
        previous_status: Option<&str>,
        timestamp: &str,
    ) {
        let req = EventPushRequest {
            events: vec![PulpoEvent::Session(SessionEvent {
                session_id: session_id.into(),
                session_name: session_name.into(),
                status: status.into(),
                previous_status: previous_status.map(str::to_owned),
                node_name: "worker-1".into(),
                output_snippet: None,
                timestamp: timestamp.into(),
                ..Default::default()
            })],
        };
        server
            .post("/api/v1/events/push")
            .add_header("authorization", format!("Bearer {token}"))
            .json(&req)
            .await
            .assert_status(axum::http::StatusCode::NO_CONTENT);
    }

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
    fn test_init_tracing_console_only() {
        // Should not panic even if called multiple times (uses try_init)
        let result = init_tracing(None, 7);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_init_tracing_with_log_dir() {
        let tmpdir = tempfile::tempdir().unwrap();
        let result = init_tracing(Some(tmpdir.path()), 7);
        assert!(result.is_ok());
        assert!(tmpdir.path().join("logs").is_dir());
    }

    #[test]
    fn test_init_tracing_degrades_on_bad_dir() {
        // Read-only path that can't be created — should fall back to console-only
        let result = init_tracing(Some(Path::new("/proc/nonexistent")), 7);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
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
    async fn test_build_app_hydrates_master_session_index_from_store() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "master-node"
port = 0
data_dir = "{}"

[auth]
token = "master-token"

[controller]
enabled = true
"#,
                data_dir.display()
            ),
        )
        .unwrap();

        let store = Store::new(data_dir.to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let persisted_id = "11111111-1111-1111-1111-111111111111";
        store
            .upsert_master_session_index_entry(&SessionIndexEntry {
                session_id: persisted_id.into(),
                node_name: "worker-1".into(),
                node_address: Some("worker-1.tail:7433".into()),
                session_name: "persisted-session".into(),
                status: "active".into(),
                command: Some("claude -p 'review'".into()),
                updated_at: "2026-04-01T20:00:00Z".into(),
            })
            .await
            .unwrap();
        store
            .touch_master_worker("worker-1", "2026-04-01T20:00:00Z")
            .await
            .unwrap();

        let cli = Cli {
            config: config_path.to_str().unwrap().into(),
            port: Some(0),
            command: None,
        };

        let (app, _addr, handle) = build_app(&cli).await.unwrap();
        let server = TestServer::new(app).unwrap();
        let resp = server.get("/api/v1/fleet/sessions").await;
        resp.assert_status_ok();
        let body: FleetSessionsResponse = resp.json();
        let persisted = body
            .sessions
            .iter()
            .find(|session| session.session.name == "persisted-session")
            .unwrap();
        assert_eq!(persisted.session.id.to_string(), persisted_id);
        assert_eq!(persisted.node_name, "worker-1");
        handle.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_restores_master_index_after_restart_and_accepts_fresh_events() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "master-node"
port = 0
data_dir = "{}"

[auth]
token = "master-token"

[controller]
enabled = true
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

        let (app1, _addr1, handle1) = build_app(&cli).await.unwrap();
        let server1 = TestServer::new(app1).unwrap();
        let worker_token = enroll_node(&server1, "worker-1").await;
        push_session_event(
            &server1,
            &worker_token,
            "11111111-1111-1111-1111-111111111111",
            "persisted-session",
            "active",
            None,
            "2026-04-02T08:00:00Z",
        )
        .await;
        handle1.shutdown();

        let (app2, _addr2, handle2) = build_app(&cli).await.unwrap();
        let server2 = TestServer::new(app2).unwrap();
        let fleet_resp = server2.get("/api/v1/fleet/sessions").await;
        fleet_resp.assert_status_ok();
        let body: FleetSessionsResponse = fleet_resp.json();
        let restored = body
            .sessions
            .iter()
            .find(|entry| entry.session.name == "persisted-session")
            .unwrap();
        assert_eq!(
            restored.session.id.to_string(),
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(restored.session.status.to_string(), "active");

        push_session_event(
            &server2,
            &worker_token,
            "11111111-1111-1111-1111-111111111111",
            "persisted-session",
            "idle",
            Some("active"),
            "2026-04-02T08:05:00Z",
        )
        .await;

        let fleet_resp = server2.get("/api/v1/fleet/sessions").await;
        fleet_resp.assert_status_ok();
        let body: FleetSessionsResponse = fleet_resp.json();
        let updated = body
            .sessions
            .iter()
            .find(|entry| entry.session.name == "persisted-session")
            .unwrap();
        assert_eq!(updated.session.status.to_string(), "idle");

        let store = Store::new(data_dir.to_str().unwrap()).await.unwrap();
        let entries = store.list_master_session_index_entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "idle");

        handle2.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_drops_pending_worker_commands_after_master_restart() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "master-node"
port = 0
data_dir = "{}"

[auth]
token = "master-token"

[controller]
enabled = true
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

        let (app1, _addr1, handle1) = build_app(&cli).await.unwrap();
        let server1 = TestServer::new(app1).unwrap();
        let worker_token = enroll_node(&server1, "worker-1").await;
        push_session_event(
            &server1,
            &worker_token,
            "session-1",
            "restart-gap",
            "active",
            None,
            "2026-04-02T08:10:00Z",
        )
        .await;
        server1
            .post("/api/v1/sessions/session-1/stop")
            .await
            .assert_status(axum::http::StatusCode::ACCEPTED);
        handle1.shutdown();

        let (app2, _addr2, handle2) = build_app(&cli).await.unwrap();
        let server2 = TestServer::new(app2).unwrap();
        let poll_resp = server2
            .get("/api/v1/node/commands")
            .add_header("authorization", format!("Bearer {worker_token}"))
            .await;
        poll_resp.assert_status_ok();
        let body: NodeCommandsResponse = poll_resp.json();
        assert!(
            body.commands.is_empty(),
            "worker command queue should be empty after controller restart"
        );
        handle2.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_restart_preserves_only_commands_already_polled_by_workers() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.toml");
        let data_dir = tmpdir.path().join("data");
        std::fs::write(
            &config_path,
            format!(
                r#"
[node]
name = "master-node"
port = 0
data_dir = "{}"

[auth]
token = "master-token"

[controller]
enabled = true
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

        let (app1, _addr1, handle1) = build_app(&cli).await.unwrap();
        let server1 = TestServer::new(app1).unwrap();
        let worker_1_token = enroll_node(&server1, "worker-1").await;
        let worker_2_token = enroll_node(&server1, "worker-2").await;
        push_session_event(
            &server1,
            &worker_1_token,
            "session-1",
            "delivered-before-restart",
            "active",
            None,
            "2026-04-02T08:15:00Z",
        )
        .await;
        push_session_event(
            &server1,
            &worker_2_token,
            "session-2",
            "pending-at-restart",
            "active",
            None,
            "2026-04-02T08:16:00Z",
        )
        .await;
        server1
            .post("/api/v1/sessions/session-1/stop")
            .await
            .assert_status(axum::http::StatusCode::ACCEPTED);
        server1
            .post("/api/v1/sessions/session-2/stop")
            .await
            .assert_status(axum::http::StatusCode::ACCEPTED);

        let worker_1_poll = server1
            .get("/api/v1/node/commands")
            .add_header("authorization", format!("Bearer {worker_1_token}"))
            .await;
        worker_1_poll.assert_status_ok();
        let body: NodeCommandsResponse = worker_1_poll.json();
        assert_eq!(body.commands.len(), 1);
        match &body.commands[0] {
            NodeCommand::StopSession { session_id, .. } => {
                assert_eq!(session_id, "session-1");
            }
            NodeCommand::CreateSession { .. } => panic!("expected StopSession"),
        }

        handle1.shutdown();

        let (app2, _addr2, handle2) = build_app(&cli).await.unwrap();
        let server2 = TestServer::new(app2).unwrap();

        let worker_1_poll_after_restart = server2
            .get("/api/v1/node/commands")
            .add_header("authorization", format!("Bearer {worker_1_token}"))
            .await;
        worker_1_poll_after_restart.assert_status_ok();
        let body: NodeCommandsResponse = worker_1_poll_after_restart.json();
        assert!(
            body.commands.is_empty(),
            "commands already drained before restart should not reappear"
        );

        let worker_2_poll_after_restart = server2
            .get("/api/v1/node/commands")
            .add_header("authorization", format!("Bearer {worker_2_token}"))
            .await;
        worker_2_poll_after_restart.assert_status_ok();
        let body: NodeCommandsResponse = worker_2_poll_after_restart.json();
        assert!(
            body.commands.is_empty(),
            "commands still pending on the master should be lost on restart"
        );

        handle2.shutdown();
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
events = ["ready", "killed"]
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
    async fn test_build_app_generates_vapid_keys() {
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

        let (_app, _addr, handle) = build_app(&cli).await.unwrap();

        // VAPID keys should have been auto-generated and saved
        let saved = config::load(config_path.to_str().unwrap()).unwrap();
        assert!(!saved.notifications.vapid.private_key.is_empty());
        assert!(!saved.notifications.vapid.public_key.is_empty());
        assert_eq!(saved.notifications.vapid.private_key.len(), 43);
        assert_eq!(saved.notifications.vapid.public_key.len(), 87);

        handle.shutdown();
    }

    #[tokio::test]
    async fn test_build_app_preserves_existing_vapid_keys() {
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
token = "existing-token"

[notifications.vapid]
private_key = "existing-priv"
public_key = "existing-pub"
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

        let (_app, _addr, handle) = build_app(&cli).await.unwrap();

        // Existing keys should be preserved
        let saved = config::load(config_path.to_str().unwrap()).unwrap();
        assert_eq!(saved.notifications.vapid.private_key, "existing-priv");
        assert_eq!(saved.notifications.vapid.public_key, "existing-pub");
        assert_eq!(saved.auth.token, "existing-token");

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
