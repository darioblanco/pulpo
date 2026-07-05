#[cfg(not(coverage))]
use std::time::Duration;

use chrono::{DateTime, Local};
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
/// Cron expressions are evaluated in the daemon's local timezone (matching
/// conventional crontab behavior). The reference time (`last_run_at` or
/// `created_at`) is converted to local time before computing the next fire.
#[cfg_attr(coverage, allow(dead_code))]
fn is_due(schedule: &Schedule) -> bool {
    is_due_at(schedule, Local::now())
}

#[cfg_attr(coverage, allow(dead_code))]
fn is_due_at(schedule: &Schedule, now: DateTime<Local>) -> bool {
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
                    .map_or_else(|_| now, |dt| dt.with_timezone(&Local))
            },
            |dt| dt.with_timezone(&Local),
        );

    // Get the next fire time after the reference, in local time
    cron.after(&reference_time)
        .next()
        .is_some_and(|next| next <= now)
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
            term_program: None,
            budget_cost_usd: None,
        };

        let result = session_manager.create_session(req).await;

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

#[cfg(test)]
mod tests {
    #[cfg(not(coverage))]
    use std::collections::HashMap;

    use super::*;
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};

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
        let now_local = Local.timestamp_opt(1_700_000_100, 0).single().unwrap();
        let schedule = Schedule {
            id: "s1".into(),
            name: "test".into(),
            cron: "* * * * *".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
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
            created_at: (now_local - ChronoDuration::hours(2))
                .with_timezone(&Utc)
                .to_rfc3339(),
        };
        assert!(is_due_at(&schedule, now_local));
    }

    #[cfg(not(coverage))]
    fn due_schedule(id: &str, name: &str, workdir: &str) -> Schedule {
        Schedule {
            id: id.into(),
            name: name.into(),
            cron: "* * * * *".into(),
            command: "echo hi".into(),
            workdir: workdir.into(),
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
            created_at: (Utc::now() - ChronoDuration::hours(2)).to_rfc3339(),
        }
    }

    #[cfg(not(coverage))]
    async fn scheduler_test_manager()
    -> (crate::session::manager::SessionManager, crate::store::Store) {
        let tmp = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        let store = crate::store::Store::new(tmp.path().to_str().unwrap())
            .await
            .unwrap();
        store.migrate().await.unwrap();
        let manager = crate::session::manager::SessionManager::new(
            std::sync::Arc::new(crate::backend::StubBackend),
            store.clone(),
            HashMap::new(),
            None,
        )
        .with_no_stale_grace();
        (manager, store)
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_fire_due_schedules_creates_session_and_records_last_run() {
        let (manager, store) = scheduler_test_manager().await;
        store
            .insert_schedule(&due_schedule("sched-1", "nightly", "/tmp"))
            .await
            .unwrap();

        fire_due_schedules(&manager, &store, None).await;

        // A session was created from the due schedule.
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].name.starts_with("nightly-"));
        assert_eq!(sessions[0].command, "echo hi");

        // last_run was recorded on the schedule.
        let after = store.get_schedule("sched-1").await.unwrap().unwrap();
        assert!(after.last_session_id.is_some());
    }

    #[cfg(not(coverage))]
    #[tokio::test]
    async fn test_fire_due_schedules_records_failure_on_bad_workdir() {
        let (manager, store) = scheduler_test_manager().await;
        // A non-existent workdir makes session creation fail (validate_workdir).
        store
            .insert_schedule(&due_schedule("sched-2", "broken", "/no/such/dir-xyz"))
            .await
            .unwrap();

        fire_due_schedules(&manager, &store, None).await;

        assert!(store.list_sessions().await.unwrap().is_empty());
        let after = store.get_schedule("sched-2").await.unwrap().unwrap();
        assert!(after.last_error.is_some(), "failure should be recorded");
        assert!(after.last_session_id.is_none());
    }

    #[test]
    fn test_is_due_recently_run() {
        // Last run 10 seconds ago with "every hour" cron — should NOT be due
        let now_local = Local.timestamp_opt(1_700_000_100, 0).single().unwrap();
        let schedule = Schedule {
            id: "s1".into(),
            name: "test".into(),
            cron: "0 * * * *".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
            ink: None,
            description: None,
            runtime: None,
            secrets: vec![],
            worktree: None,
            worktree_base: None,
            enabled: true,
            last_run_at: Some(
                (now_local - ChronoDuration::seconds(10))
                    .with_timezone(&Utc)
                    .to_rfc3339(),
            ),
            last_session_id: Some("prev".into()),
            last_attempted_at: None,
            last_error: None,
            created_at: (now_local - ChronoDuration::hours(24))
                .with_timezone(&Utc)
                .to_rfc3339(),
        };
        assert!(!is_due_at(&schedule, now_local));
    }

    #[test]
    fn test_is_due_invalid_cron() {
        let now_local = Local.timestamp_opt(1_700_000_100, 0).single().unwrap();
        let schedule = Schedule {
            id: "s1".into(),
            name: "test".into(),
            cron: "invalid".into(),
            command: "echo".into(),
            workdir: "/tmp".into(),
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
            created_at: now_local.with_timezone(&Utc).to_rfc3339(),
        };
        assert!(!is_due_at(&schedule, now_local));
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
            created_at: (Utc::now() - ChronoDuration::hours(1)).to_rfc3339(),
        };
        assert!(is_due(&schedule));
    }
}
