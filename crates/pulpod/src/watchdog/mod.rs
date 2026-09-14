mod budget;
mod git;
mod idle;
mod intervention;
mod metadata;
pub mod output_patterns;

use std::sync::Arc;
use std::time::Duration;

use crate::harness::{HarnessRegistry, HarnessSignals};
use idle::check_idle_sessions;
#[cfg(test)]
use idle::{
    check_session_idle, handle_active_session, handle_idle_session, handle_session_ready,
    sweep_ready_exit_code,
};
pub(crate) use metadata::refresh_exact_usage;
use metadata::{build_session_event, detect_and_store_output_metadata};
pub use output_patterns::detect_waiting_for_input;
use pulpo_common::event::PulpoEvent;
use tokio::sync::broadcast;
use tracing::info;

use pulpo_common::session::Session;

use crate::backend::Backend;
use crate::store::Store;
use git::update_git_info;

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

/// True when a harness adapter owns this session's state — its `harness_last_event_at`
/// is set, meaning lifecycle hook events are flowing for it. The budget/burn breakers,
/// git telemetry, PR detection, and `idle_timeout` still apply regardless (see
/// `watchdog::idle::handle_idle_session`, which runs unconditionally for every
/// session). Sessions without events (generic harness, or a harness whose hooks
/// failed to install) keep today's heuristics unchanged, since `harness_last_event_at`
/// never gets set for them.
///
/// This alone doesn't say *which* heuristics to skip — see [`owned_signals`] for the
/// granular version (spec §5): an adapter may own lifecycle signals (turn/session
/// boundaries, permission/idle prompts) without owning error/rate-limit detection
/// (Codex has no hook for either), in which case those two heuristics must keep
/// running even while lifecycle events flow.
pub(super) const fn harness_owns_state(session: &Session) -> bool {
    session.harness_last_event_at.is_some()
}

/// Process-wide, stateless registry used only to resolve [`HarnessSignals`] by a
/// session's own harness id. Cheap to construct (a handful of `Arc::new` calls, no
/// I/O) but built once via `LazyLock` rather than per call.
static HARNESS_REGISTRY: std::sync::LazyLock<HarnessRegistry> =
    std::sync::LazyLock::new(HarnessRegistry::default);

/// Which scrollback-heuristic signals remain safe for the watchdog to apply to this
/// session — see [`HarnessSignals`]. Combines "are this session's hook events
/// actually flowing" ([`harness_owns_state`]) with "which signals does its own
/// adapter replace" (`crate::harness::HarnessAdapter::owned_signals`).
pub(super) fn owned_signals(session: &Session) -> HarnessSignals {
    HARNESS_REGISTRY.owned_signals_for(session.harness.as_deref(), harness_owns_state(session))
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

/// Runtime configuration for the watchdog loop, read once at startup.
#[derive(Debug, Clone)]
pub struct WatchdogRuntimeConfig {
    pub interval: Duration,
    pub idle: IdleConfig,
    /// Extra user-configured patterns for waiting-for-input detection.
    pub extra_waiting_patterns: Vec<String>,
}

/// Context for handling agent-ready transitions (status update + events).
#[cfg_attr(coverage, allow(dead_code))]
pub struct ReadyContext {
    pub event_tx: Option<broadcast::Sender<PulpoEvent>>,
    pub node_name: String,
}

async fn run_watchdog_tick(
    backend: &Arc<dyn Backend>,
    store: &Store,
    cfg: &WatchdogRuntimeConfig,
    ready_ctx: &ReadyContext,
) {
    // `check_idle_sessions` runs first: it's what refreshes each session's
    // `session_cost_usd` metadata for this tick (via `detect_and_store_output_metadata`
    // reading the agent's own transcript). Enforcing budgets before that would judge
    // every session against last tick's cost — a real cost breach would only ever be
    // caught one tick late. Running idle detection first means the budget check below
    // always sees this tick's own fresh numbers.
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

    budget::enforce_budgets(backend, store, ready_ctx).await;

    update_git_info(store).await;
}

/// Runs the watchdog loop that checks per-session breakers (idle, budget) on a
/// fixed tick and intervenes when one trips.
///
/// `cfg` is read once at startup — there is no live config-reload path (the
/// `PUT /api/v1/watchdog` hot-reload endpoint was removed along with the rest of
/// the config-editing API; see `docs/reference/config.md`). Changing `[watchdog]`
/// in the config file takes effect on the next `pulpod` restart.
pub async fn run_watchdog_loop(
    backend: Arc<dyn Backend>,
    store: Store,
    cfg: WatchdogRuntimeConfig,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ready_ctx: ReadyContext,
) {
    let mut tick = tokio::time::interval(cfg.interval);
    tick.tick().await; // first tick completes immediately

    loop {
        tokio::select! {
            _ = tick.tick() => {
                run_watchdog_tick(&backend, &store, &cfg, &ready_ctx).await;
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
