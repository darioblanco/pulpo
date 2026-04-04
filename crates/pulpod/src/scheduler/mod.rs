#[cfg(not(coverage))]
use std::time::Duration;

use chrono::Local;
use cron::Schedule as CronSchedule;
#[cfg(not(coverage))]
use pulpo_common::api::CreateSessionRequest;
use pulpo_common::api::Schedule;
#[cfg(not(coverage))]
use pulpo_common::event::PulpoEvent;
#[cfg(not(coverage))]
use tokio::sync::{broadcast, watch};
#[cfg(not(coverage))]
use tracing::{debug, info, warn};

#[cfg(not(coverage))]
use crate::remote::{apply_remote_auth, remote_client, resolve_peer_target};
#[cfg(not(coverage))]
use crate::session::manager::SessionManager;
#[cfg(not(coverage))]
use crate::store::Store;
#[cfg(not(coverage))]
use crate::{config::NodeRole, peers::PeerRegistry};

#[cfg(coverage)]
use crate::config::NodeRole;

/// Normalize a cron expression to the 7-field format expected by the `cron` crate.
/// Accepts standard 5-field (`min hour dom month dow`) and prepends `0` for seconds
/// and appends `*` for year. Also accepts 6-field (with seconds) and 7-field (full).
fn normalize_cron(expr: &str) -> String {
    let field_count = expr.split_whitespace().count();
    match field_count {
        5 => format!("0 {expr} *"),
        6 => format!("{expr} *"),
        _ => expr.to_owned(),
    }
}

/// Validate a cron expression. Returns an error message if invalid.
/// Accepts standard 5-field cron expressions (e.g., `0 3 * * *`).
pub fn validate_cron(expr: &str) -> Result<(), String> {
    normalize_cron(expr)
        .parse::<CronSchedule>()
        .map(|_| ())
        .map_err(|e| format!("invalid cron expression: {e}"))
}

/// Check if a schedule is due to fire now.
/// Cron expressions are evaluated in the daemon's local timezone (matching
/// conventional crontab behavior). The reference time (`last_run_at` or
/// `created_at`) is converted to local time before computing the next fire.
#[cfg_attr(coverage, allow(dead_code))]
fn is_due(schedule: &Schedule) -> bool {
    let Ok(cron) = normalize_cron(&schedule.cron).parse::<CronSchedule>() else {
        return false;
    };

    // Parse the reference time and convert to local timezone so that cron
    // fields (hour, minute, etc.) match the daemon machine's wall clock.
    let reference_time = schedule
        .last_run_at
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map_or_else(
            || {
                chrono::DateTime::parse_from_rfc3339(&schedule.created_at)
                    .map_or_else(|_| Local::now(), |dt| dt.with_timezone(&Local))
            },
            |dt| dt.with_timezone(&Local),
        );

    // Get the next fire time after the reference, in local time
    cron.after(&reference_time)
        .next()
        .is_some_and(|next| next <= Local::now())
}

/// Run the scheduler loop. Ticks every 60 seconds and fires due schedules.
#[cfg(not(coverage))]
pub async fn run_scheduler_loop(
    session_manager: SessionManager,
    store: Store,
    role: NodeRole,
    local_node_name: String,
    peer_registry: PeerRegistry,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.tick().await; // first tick completes immediately

    loop {
        tokio::select! {
            _ = tick.tick() => {
                fire_due_schedules(
                    &session_manager,
                    &store,
                    role,
                    &local_node_name,
                    &peer_registry,
                    event_tx.as_ref(),
                )
                .await;
            }
            _ = shutdown_rx.changed() => {
                info!("Scheduler shutting down");
                break;
            }
        }
    }
}

#[cfg(not(coverage))]
async fn fire_due_schedules(
    session_manager: &SessionManager,
    store: &Store,
    role: NodeRole,
    local_node_name: &str,
    peer_registry: &PeerRegistry,
    _event_tx: Option<&broadcast::Sender<PulpoEvent>>,
) {
    let schedules = match store.list_schedules().await {
        Ok(s) => s,
        Err(e) => {
            warn!("Scheduler: failed to list schedules: {e}");
            return;
        }
    };

    for schedule in schedules {
        if !schedule.enabled {
            continue;
        }
        if !should_fire_schedule(role, local_node_name, schedule.target_node.as_deref()) {
            continue;
        }
        if !is_due(&schedule) {
            continue;
        }

        debug!(schedule_name = %schedule.name, "Schedule is due, firing");

        // Build session name from schedule name + timestamp suffix
        let session_name = format!("{}-{}", schedule.name, Local::now().format("%Y%m%d-%H%M"));

        let runtime = schedule.runtime.as_deref().map(|r| {
            r.parse::<pulpo_common::session::Runtime>().unwrap_or_else(|_| {
                warn!(schedule_name = %schedule.name, runtime = r, "Unknown runtime, falling back to tmux");
                pulpo_common::session::Runtime::Tmux
            })
        });

        let secrets = if schedule.secrets.is_empty() {
            None
        } else {
            Some(schedule.secrets.clone())
        };

        let req = CreateSessionRequest {
            name: session_name,
            workdir: Some(schedule.workdir.clone()),
            command: if schedule.command.is_empty() {
                None
            } else {
                Some(schedule.command.clone())
            },
            ink: schedule.ink.clone(),
            description: schedule.description.clone(),
            metadata: None,
            idle_threshold_secs: None,
            worktree: schedule.worktree,
            worktree_base: schedule.worktree_base.clone(),
            runtime,
            secrets,
            target_node: None,
        };

        let result = dispatch_schedule_create(
            session_manager,
            role,
            local_node_name,
            peer_registry,
            &schedule,
            req,
        )
        .await;

        match result {
            Ok(session) => {
                info!(
                    schedule_name = %schedule.name,
                    session_name = %session.name,
                    session_id = %session.id,
                    "Schedule fired successfully"
                );
                if let Err(e) = store
                    .update_schedule_last_run(&schedule.id, &session.id.to_string())
                    .await
                {
                    warn!(
                        schedule_name = %schedule.name,
                        "Failed to update schedule last_run: {e}"
                    );
                }
            }
            Err(e) => {
                warn!(
                    schedule_name = %schedule.name,
                    "Schedule fire failed: {e}"
                );
                if let Err(err) = store
                    .record_schedule_failure(&schedule.id, &e.to_string())
                    .await
                {
                    warn!(
                        schedule_name = %schedule.name,
                        "Failed to record schedule failure: {err}"
                    );
                }
            }
        }
    }
}

fn should_fire_schedule(role: NodeRole, local_node_name: &str, target_node: Option<&str>) -> bool {
    if let Some(target) = target_node
        && role != NodeRole::Controller
        && target != local_node_name
    {
        return false;
    }
    true
}

#[cfg(not(coverage))]
async fn dispatch_schedule_create(
    session_manager: &SessionManager,
    role: NodeRole,
    local_node_name: &str,
    peer_registry: &PeerRegistry,
    schedule: &Schedule,
    req: CreateSessionRequest,
) -> anyhow::Result<pulpo_common::session::Session> {
    match schedule.target_node.as_deref() {
        Some(target_node) if role != NodeRole::Controller && target_node != local_node_name => Err(
            anyhow::anyhow!("target_node requires controller mode (got {role:?})"),
        ),
        Some(target_node) if role == NodeRole::Controller && target_node != local_node_name => {
            create_remote_scheduled_session(peer_registry, schedule.name.as_str(), target_node, req)
                .await
        }
        _ => session_manager.create_session(req).await,
    }
}

#[cfg(not(coverage))]
async fn create_remote_scheduled_session(
    peer_registry: &PeerRegistry,
    schedule_name: &str,
    target_node: &str,
    req: CreateSessionRequest,
) -> anyhow::Result<pulpo_common::session::Session> {
    let Some(target) = resolve_peer_target(peer_registry, target_node).await else {
        warn!(
            schedule_name = %schedule_name,
            target_node = %target_node,
            "Schedule target node not found"
        );
        return Err(anyhow::anyhow!("target node not found: {target_node}"));
    };
    let client = remote_client()
        .map_err(|e| anyhow::anyhow!("failed to build scheduler HTTP client: {e}"))?;
    let request = apply_remote_auth(
        client
            .post(format!("{}/api/v1/sessions", target.base_url))
            .json(&req),
        target.token.as_deref(),
    );

    match request.send().await {
        Ok(resp) if resp.status().is_success() => resp
            .json::<pulpo_common::api::CreateSessionResponse>()
            .await
            .map(|body| body.session)
            .map_err(|e| anyhow::anyhow!("failed to parse remote schedule create response: {e}")),
        Ok(resp) => {
            let status = resp.status();
            let message = match resp.json::<pulpo_common::api::ErrorResponse>().await {
                Ok(body) => body.error,
                Err(_) => format!("node responded with {status}"),
            };
            Err(anyhow::anyhow!(message))
        }
        Err(e) => Err(anyhow::anyhow!(
            "failed to reach node {}: {e}",
            target.node_name
        )),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(coverage))]
    use std::collections::HashMap;

    use super::*;
    #[cfg(not(coverage))]
    use crate::peers::PeerRegistry;
    use chrono::Utc;
    #[cfg(not(coverage))]
    use pulpo_common::{api::CreateSessionRequest, peer::PeerEntry};

    #[test]
    fn test_normalize_cron_5_fields() {
        assert_eq!(normalize_cron("0 3 * * *"), "0 0 3 * * * *");
    }

    #[test]
    fn test_normalize_cron_6_fields() {
        assert_eq!(normalize_cron("0 0 3 * * *"), "0 0 3 * * * *");
    }

    #[test]
    fn test_normalize_cron_7_fields() {
        assert_eq!(normalize_cron("0 0 3 * * * *"), "0 0 3 * * * *");
    }

    #[test]
    fn test_validate_cron_valid() {
        assert!(validate_cron("0 3 * * *").is_ok());
        assert!(validate_cron("*/5 * * * *").is_ok());
        assert!(validate_cron("0 0 * * SUN").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        assert!(validate_cron("not a cron").is_err());
        assert!(validate_cron("").is_err());
    }

    #[test]
    fn test_is_due_never_run() {
        // Schedule created 2 hours ago with "every minute" cron — should be due
        let schedule = Schedule {
            id: "s1".into(),
            name: "test".into(),
            cron: "* * * * *".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
            target_node: None,
            ink: None,
            description: None,
            runtime: None,
            secrets: vec![],
            worktree: None,
            worktree_base: None,
            enabled: true,
            last_run_at: None,
            last_session_id: None,
            last_attempted_at: None,
            last_error: None,
            created_at: (Utc::now() - chrono::Duration::hours(2)).to_rfc3339(),
        };
        assert!(is_due(&schedule));
    }

    #[test]
    fn test_is_due_recently_run() {
        // Last run 10 seconds ago with "every hour" cron — should NOT be due
        let schedule = Schedule {
            id: "s1".into(),
            name: "test".into(),
            cron: "0 * * * *".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
            target_node: None,
            ink: None,
            description: None,
            runtime: None,
            secrets: vec![],
            worktree: None,
            worktree_base: None,
            enabled: true,
            last_run_at: Some((Utc::now() - chrono::Duration::seconds(10)).to_rfc3339()),
            last_session_id: Some("prev".into()),
            last_attempted_at: None,
            last_error: None,
            created_at: (Utc::now() - chrono::Duration::hours(24)).to_rfc3339(),
        };
        assert!(!is_due(&schedule));
    }

    #[test]
    fn test_is_due_invalid_cron() {
        let schedule = Schedule {
            id: "s1".into(),
            name: "test".into(),
            cron: "invalid".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
            target_node: None,
            ink: None,
            description: None,
            runtime: None,
            secrets: vec![],
            worktree: None,
            worktree_base: None,
            enabled: true,
            last_run_at: None,
            last_session_id: None,
            last_attempted_at: None,
            last_error: None,
            created_at: Utc::now().to_rfc3339(),
        };
        assert!(!is_due(&schedule));
    }

    #[test]
    fn test_is_due_disabled_still_checks() {
        // is_due doesn't check enabled — that's the caller's job
        let schedule = Schedule {
            id: "s1".into(),
            name: "test".into(),
            cron: "* * * * *".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
            target_node: None,
            ink: None,
            description: None,
            runtime: None,
            secrets: vec![],
            worktree: None,
            worktree_base: None,
            enabled: false,
            last_run_at: None,
            last_session_id: None,
            last_attempted_at: None,
            last_error: None,
            created_at: (Utc::now() - chrono::Duration::hours(1)).to_rfc3339(),
        };
        assert!(is_due(&schedule));
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_create_remote_scheduled_session_target_not_found() {
        let registry = PeerRegistry::new(&HashMap::new());
        let err = create_remote_scheduled_session(
            &registry,
            "nightly-review",
            "missing-node",
            CreateSessionRequest {
                name: "nightly-review".into(),
                workdir: Some("/repo".into()),
                metadata: None,
                command: Some("claude code".into()),
                description: None,
                ink: None,
                idle_threshold_secs: None,
                worktree: None,
                worktree_base: None,
                runtime: None,
                secrets: None,
                target_node: None,
            },
        )
        .await
        .unwrap_err();

        assert_eq!(err.to_string(), "target node not found: missing-node");
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_create_remote_scheduled_session_unreachable_worker() {
        let configured = HashMap::from([(
            "node-1".to_owned(),
            PeerEntry::Full {
                address: "127.0.0.1:9".into(),
                token: Some("secret-token".into()),
            },
        )]);
        let registry = PeerRegistry::new(&configured);
        let err = create_remote_scheduled_session(
            &registry,
            "nightly-review",
            "node-1",
            CreateSessionRequest {
                name: "nightly-review".into(),
                workdir: Some("/repo".into()),
                metadata: None,
                command: Some("claude code".into()),
                description: None,
                ink: None,
                idle_threshold_secs: None,
                worktree: None,
                worktree_base: None,
                runtime: None,
                secrets: None,
                target_node: None,
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("failed to reach node node-1"));
    }
}
