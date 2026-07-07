mod adopt;
mod budget;
mod burn;
mod git;
mod idle;
mod intervention;
pub mod memory;
mod metadata;
pub mod output_patterns;

use std::sync::Arc;
use std::time::Duration;

use adopt::adopt_tmux_sessions;
#[cfg(test)]
use adopt::classify_adopted_process;
use idle::{check_idle_sessions, cleanup_ready_sessions};
#[cfg(test)]
use idle::{check_session_idle, handle_active_session, handle_idle_session, handle_session_ready};
use memory::MemoryReader;
#[cfg(test)]
use memory::MemorySnapshot;
use metadata::{build_session_event, detect_and_store_output_metadata};
pub use output_patterns::detect_waiting_for_input;
use pulpo_common::event::PulpoEvent;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use pulpo_common::session::Session;

use crate::backend::Backend;
use crate::store::Store;
use git::update_git_info;
use intervention::intervene;

/// The marker emitted by the agent wrapper when the agent process exits.
const AGENT_EXIT_MARKER: &str = "[pulpo] Agent exited";

/// Check if the terminal output contains the agent exit marker.
pub fn detect_agent_exited(output: &str) -> bool {
    output.contains(AGENT_EXIT_MARKER)
}

/// Resolve the backend session ID from a session, falling back to session name.
fn resolve_backend_id(session: &Session, backend: &dyn Backend) -> String {
    session
        .backend_session_id
        .clone()
        .unwrap_or_else(|| backend.session_id(&session.name))
}

/// List all sessions from the store, warning (with the caller's `context` label)
/// and returning an empty list on error so watchdog checks degrade gracefully
/// instead of aborting the tick.
#[cfg_attr(coverage, allow(unused_variables))]
async fn list_sessions_or_warn(store: &Store, context: &str) -> Vec<Session> {
    match store.list_sessions().await {
        Ok(sessions) => sessions,
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("{context}: failed to list sessions: {error}");
            Vec::new()
        }
    }
}

/// Action to take when a session is detected as idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    Alert,
    Kill,
}

/// Action to take when a session crosses a burn-velocity ceiling.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BurnAction {
    /// Emit a `usage_alert.burn_ceiling` event only (default).
    #[default]
    Alert,
    /// Emit the alert and stop the session via the intervention path.
    Stop,
}

impl BurnAction {
    /// Map a config `burn_action` string to the parsed action. Anything other
    /// than `"stop"` is treated as the safe default (`Alert`); the config layer
    /// validates the string up front, so this only ever sees `alert`/`stop`.
    #[must_use]
    pub fn from_config_str(action: &str) -> Self {
        if action == "stop" {
            Self::Stop
        } else {
            Self::Alert
        }
    }
}

/// Configuration for the burn-velocity governor.
///
/// A session is over-ceiling when its lifetime-average cost rate exceeds
/// `ceiling_usd_per_hour` (when set) **or** its token rate exceeds
/// `ceiling_tokens_per_hour` (when set). The check is skipped entirely when both
/// ceilings are `None`. The ceiling is global by design — a runaway/loop detector
/// is naturally fleet-wide, not per-session.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct BurnConfig {
    pub ceiling_usd_per_hour: Option<f64>,
    pub ceiling_tokens_per_hour: Option<u64>,
    pub action: BurnAction,
}

impl BurnConfig {
    /// Build a runtime [`BurnConfig`] from the persisted watchdog config fields.
    #[must_use]
    pub fn from_watchdog_config(cfg: &crate::config::WatchdogConfig) -> Self {
        Self {
            ceiling_usd_per_hour: cfg.burn_ceiling_usd_per_hour,
            ceiling_tokens_per_hour: cfg.burn_ceiling_tokens_per_hour,
            action: BurnAction::from_config_str(&cfg.burn_action),
        }
    }
}

/// Configuration for idle session detection.
#[derive(Debug, Clone)]
pub struct IdleConfig {
    pub enabled: bool,
    pub timeout_secs: u64,
    pub action: IdleAction,
    /// Seconds of unchanged output before Active→Idle transition.
    pub threshold_secs: u64,
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_secs: 600,
            action: IdleAction::Alert,
            threshold_secs: 60,
        }
    }
}

/// Runtime configuration for the watchdog loop, updated via watch channel.
#[derive(Debug, Clone)]
pub struct WatchdogRuntimeConfig {
    pub threshold: u8,
    pub interval: Duration,
    pub breach_count: u32,
    pub idle: IdleConfig,
    /// Seconds after Ready before tmux shell is killed (0 = disabled).
    pub ready_ttl_secs: u64,
    /// Auto-adopt external tmux sessions into pulpo management.
    pub adopt_tmux: bool,
    /// Extra user-configured patterns for waiting-for-input detection.
    pub extra_waiting_patterns: Vec<String>,
    /// Burn-velocity governor settings (cost/token rate ceilings + action).
    pub burn: BurnConfig,
}

/// Context for handling agent-ready transitions (status update + events).
#[cfg_attr(coverage, allow(dead_code))]
pub struct ReadyContext {
    pub event_tx: Option<broadcast::Sender<PulpoEvent>>,
    pub node_name: String,
}

async fn refresh_watchdog_ticker(
    tick: &mut tokio::time::Interval,
    current_interval: &mut Duration,
    next_interval: Duration,
) {
    if next_interval != *current_interval {
        info!(
            old_interval_secs = current_interval.as_secs(),
            new_interval_secs = next_interval.as_secs(),
            "Watchdog interval changed, resetting ticker"
        );
        *current_interval = next_interval;
        *tick = tokio::time::interval(next_interval);
        tick.tick().await;
    }
}

fn update_breach_counter(usage: u8, threshold: u8, consecutive_breaches: &mut u32) -> bool {
    if usage >= threshold {
        *consecutive_breaches += 1;
        true
    } else {
        if *consecutive_breaches > 0 {
            info!(
                usage,
                threshold, "Memory pressure subsided, resetting breach counter"
            );
        }
        *consecutive_breaches = 0;
        false
    }
}

async fn run_memory_check(
    backend: &Arc<dyn Backend>,
    store: &Store,
    reader: &dyn MemoryReader,
    cfg: &WatchdogRuntimeConfig,
    consecutive_breaches: &mut u32,
    ready_ctx: &ReadyContext,
) {
    match reader.read_memory() {
        Ok(snapshot) => {
            let usage = snapshot.usage_percent();
            debug!(
                usage,
                threshold = cfg.threshold,
                consecutive_breaches,
                "Memory check"
            );

            if update_breach_counter(usage, cfg.threshold, consecutive_breaches) {
                warn!(
                    usage,
                    threshold = cfg.threshold,
                    consecutive_breaches,
                    breach_count = cfg.breach_count,
                    available_mb = snapshot.available_mb,
                    total_mb = snapshot.total_mb,
                    "Memory pressure detected"
                );

                if *consecutive_breaches >= cfg.breach_count {
                    intervene(backend, store, &snapshot, ready_ctx).await;
                    *consecutive_breaches = 0;
                }
            }
        }
        #[allow(unused_variables)]
        Err(error) => {
            coverage_warn!("Failed to read memory: {error}");
        }
    }
}

async fn run_watchdog_tick(
    backend: &Arc<dyn Backend>,
    store: &Store,
    reader: &dyn MemoryReader,
    cfg: &WatchdogRuntimeConfig,
    ready_ctx: &ReadyContext,
    consecutive_breaches: &mut u32,
) {
    run_memory_check(backend, store, reader, cfg, consecutive_breaches, ready_ctx).await;

    budget::enforce_budgets(backend, store, ready_ctx).await;

    burn::enforce_burn_ceiling(backend, store, ready_ctx, &cfg.burn).await;

    if cfg.idle.enabled {
        check_idle_sessions(
            backend,
            store,
            &cfg.idle,
            ready_ctx,
            &cfg.extra_waiting_patterns,
        )
        .await;
    }

    if cfg.ready_ttl_secs > 0 {
        cleanup_ready_sessions(backend, store, cfg.ready_ttl_secs).await;
    }

    if cfg.adopt_tmux {
        adopt_tmux_sessions(backend, store, ready_ctx).await;
    }

    update_git_info(store).await;
}

/// Runs the watchdog loop that monitors system memory and intervenes when sustained pressure
/// is detected. Kills running sessions after `breach_count` consecutive checks above `threshold`.
///
/// The loop dynamically picks up config changes sent via the `config_rx` watch channel.
pub async fn run_watchdog_loop(
    backend: Arc<dyn Backend>,
    store: Store,
    reader: Box<dyn MemoryReader>,
    config_rx: tokio::sync::watch::Receiver<WatchdogRuntimeConfig>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ready_ctx: ReadyContext,
) {
    let initial = config_rx.borrow().clone();
    let mut current_interval = initial.interval;
    let mut tick = tokio::time::interval(current_interval);
    tick.tick().await; // first tick completes immediately
    let mut consecutive_breaches: u32 = 0;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let cfg = config_rx.borrow().clone();
                refresh_watchdog_ticker(&mut tick, &mut current_interval, cfg.interval).await;
                run_watchdog_tick(
                    &backend,
                    &store,
                    reader.as_ref(),
                    &cfg,
                    &ready_ctx,
                    &mut consecutive_breaches,
                )
                .await;
            }
            _ = shutdown_rx.changed() => {
                info!("Watchdog shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests;
