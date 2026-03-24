#[cfg(not(coverage))]
use std::time::Duration;

use chrono::Utc;
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
use crate::session::manager::SessionManager;
#[cfg(not(coverage))]
use crate::store::Store;

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
/// A schedule is due if its next fire time (after `last_run_at` or `created_at`) is in the past.
#[cfg_attr(coverage, allow(dead_code))]
fn is_due(schedule: &Schedule) -> bool {
    let Ok(cron) = normalize_cron(&schedule.cron).parse::<CronSchedule>() else {
        return false;
    };

    let reference_time = schedule
        .last_run_at
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map_or_else(
            || {
                chrono::DateTime::parse_from_rfc3339(&schedule.created_at)
                    .map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc))
            },
            |dt| dt.with_timezone(&Utc),
        );

    // Get the next fire time after the reference
    cron.after(&reference_time)
        .next()
        .is_some_and(|next| next <= Utc::now())
}

/// Run the scheduler loop. Ticks every 60 seconds and fires due schedules.
#[cfg(not(coverage))]
pub async fn run_scheduler_loop(
    session_manager: SessionManager,
    store: Store,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.tick().await; // first tick completes immediately

    loop {
        tokio::select! {
            _ = tick.tick() => {
                fire_due_schedules(&session_manager, &store, event_tx.as_ref()).await;
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
        if !is_due(&schedule) {
            continue;
        }

        debug!(schedule_name = %schedule.name, "Schedule is due, firing");

        // Build session name from schedule name + timestamp suffix
        let session_name = format!("{}-{}", schedule.name, Utc::now().format("%Y%m%d-%H%M"));

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
            worktree: None,
            worktree_base: None,
            runtime: None,
            secrets: None,
        };

        match session_manager.create_session(req).await {
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
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            enabled: true,
            last_run_at: None,
            last_session_id: None,
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
            enabled: true,
            last_run_at: Some((Utc::now() - chrono::Duration::seconds(10)).to_rfc3339()),
            last_session_id: Some("prev".into()),
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
            enabled: true,
            last_run_at: None,
            last_session_id: None,
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
            enabled: false,
            last_run_at: None,
            last_session_id: None,
            created_at: (Utc::now() - chrono::Duration::hours(1)).to_rfc3339(),
        };
        assert!(is_due(&schedule));
    }
}
