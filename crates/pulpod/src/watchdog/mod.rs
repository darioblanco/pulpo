mod adopt;
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

/// Action to take when a session is detected as idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    Alert,
    Kill,
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
}

/// Context for handling agent-ready transitions (status update + events).
#[cfg_attr(coverage, allow(dead_code))]
pub struct ReadyContext {
    pub event_tx: Option<broadcast::Sender<PulpoEvent>>,
    pub node_name: String,
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

                // If interval changed, recreate the ticker
                if cfg.interval != current_interval {
                    info!(
                        old_interval_secs = current_interval.as_secs(),
                        new_interval_secs = cfg.interval.as_secs(),
                        "Watchdog interval changed, resetting ticker"
                    );
                    current_interval = cfg.interval;
                    tick = tokio::time::interval(current_interval);
                    tick.tick().await; // consume immediate first tick
                }

                match reader.read_memory() {
                    Ok(snapshot) => {
                        let usage = snapshot.usage_percent();
                        debug!(usage, threshold = cfg.threshold, consecutive_breaches, "Memory check");

                        if usage >= cfg.threshold {
                            consecutive_breaches += 1;
                            warn!(
                                usage,
                                threshold = cfg.threshold,
                                consecutive_breaches,
                                breach_count = cfg.breach_count,
                                available_mb = snapshot.available_mb,
                                total_mb = snapshot.total_mb,
                                "Memory pressure detected"
                            );

                            if consecutive_breaches >= cfg.breach_count {
                                intervene(&backend, &store, &snapshot).await;
                                consecutive_breaches = 0;
                            }
                        } else {
                            if consecutive_breaches > 0 {
                                info!(
                                    usage,
                                    threshold = cfg.threshold,
                                    "Memory pressure subsided, resetting breach counter"
                                );
                            }
                            consecutive_breaches = 0;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read memory: {e}");
                    }
                }

                // Idle + ready detection runs on every tick, independent of memory checks
                if cfg.idle.enabled {
                    check_idle_sessions(&backend, &store, &cfg.idle, &ready_ctx, &cfg.extra_waiting_patterns).await;
                }

                // Clean up ready sessions whose tmux shell has exceeded the TTL
                if cfg.ready_ttl_secs > 0 {
                    cleanup_ready_sessions(&backend, &store, cfg.ready_ttl_secs).await;
                }

                // Auto-adopt external tmux sessions
                if cfg.adopt_tmux {
                    adopt_tmux_sessions(&backend, &store, &ready_ctx).await;
                }

                // Update git branch/commit info for active sessions
                update_git_info(&store).await;
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
