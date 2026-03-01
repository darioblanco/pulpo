use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use pulpo_common::api::CreateSessionRequest;
use pulpo_common::event::{PulpoEvent, ScheduleEvent};
use pulpo_common::schedule::{
    ConcurrencyPolicy, ExecutionStatus, Schedule, ScheduleExecution, ScheduleStatus,
};
use pulpo_common::session::SessionStatus;
use tokio::sync::{broadcast, watch};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::session::manager::SessionManager;
use crate::store::Store;

/// Validates a 5-field cron expression by converting to 7-field and parsing.
/// Returns the validated 5-field expression on success.
pub fn validate_cron(expr: &str) -> Result<()> {
    let seven = to_seven_field(expr);
    seven
        .parse::<cron::Schedule>()
        .map_err(|e| anyhow::anyhow!("invalid cron expression: {e}"))?;
    Ok(())
}

/// Converts a 5-field user cron expression to 7-field (prepend seconds=0, append year=*).
fn to_seven_field(expr: &str) -> String {
    format!("0 {expr} *")
}

/// Computes the next run time from the given cron expression, starting after `after`.
///
/// The year field is always `*` (see `to_seven_field`), so the iterator is infinite
/// and `next()` always returns `Some`. We use `context()` for safety.
pub fn compute_next_run(
    cron_expr: &str,
    after: chrono::DateTime<Utc>,
) -> Result<chrono::DateTime<Utc>> {
    let seven = to_seven_field(cron_expr);
    let schedule: cron::Schedule = seven
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid cron expression: {e}"))?;
    schedule
        .after(&after)
        .next()
        .context("cron expression has no future occurrences")
}

/// Expands template variables in a prompt string.
///
/// Supported variables:
/// - `{date}` → UTC date (`YYYY-MM-DD`)
/// - `{datetime}` → UTC ISO 8601
/// - `{schedule}` → schedule name
/// - `{run}` → run number (`execution_count + 1`)
pub fn expand_template(template: &str, schedule_name: &str, run_number: u32) -> String {
    let now = Utc::now();
    template
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{datetime}", &now.to_rfc3339())
        .replace("{schedule}", schedule_name)
        .replace("{run}", &run_number.to_string())
}

/// Builds a `CreateSessionRequest` from a schedule, with prompt template expansion.
pub fn build_session_request(schedule: &Schedule, run_number: u32) -> CreateSessionRequest {
    let prompt = expand_template(&schedule.prompt, &schedule.name, run_number);
    let name = format!("{}-{run_number}", schedule.name);

    CreateSessionRequest {
        name: Some(name),
        workdir: schedule.workdir.clone(),
        provider: Some(schedule.provider),
        prompt,
        mode: Some(schedule.mode),
        guard_preset: schedule.guard_preset.as_ref().and_then(|s| s.parse().ok()),
        guard_config: schedule.guard_config.clone(),
        model: schedule.model.clone(),
        allowed_tools: schedule.allowed_tools.clone(),
        system_prompt: schedule.system_prompt.clone(),
        metadata: schedule.metadata.clone(),
        persona: schedule.persona.clone(),
        max_turns: schedule.max_turns,
        max_budget_usd: schedule.max_budget_usd,
        output_format: schedule.output_format.clone(),
    }
}

/// Determines whether the schedule should fire, based on its concurrency policy
/// and whether its last session is still running.
///
/// Returns:
/// - `Ok(true)` — proceed to spawn
/// - `Ok(false)` — skip (recorded as Skipped execution by caller)
/// - Kills the running session for `Replace` policy before returning `Ok(true)`
pub async fn evaluate_concurrency(
    schedule: &Schedule,
    session_manager: &SessionManager,
) -> Result<bool> {
    let Some(last_session_id) = schedule.last_session_id else {
        return Ok(true); // No prior session — always fire
    };

    let session = session_manager
        .get_session(&last_session_id.to_string())
        .await?;

    let is_running = session
        .as_ref()
        .is_some_and(|s| s.status == SessionStatus::Running);

    if !is_running {
        return Ok(true);
    }

    match schedule.concurrency {
        ConcurrencyPolicy::Skip => Ok(false),
        ConcurrencyPolicy::Allow => Ok(true),
        ConcurrencyPolicy::Replace => {
            session_manager
                .kill_session(&last_session_id.to_string())
                .await?;
            Ok(true)
        }
    }
}

/// Runs the scheduler loop. Same shutdown pattern as the watchdog.
///
/// Each tick: loads active schedules, checks `next_run_at <= now`,
/// fires due ones sequentially via `SessionManager::create_session()`.
pub async fn run_scheduler_loop(
    session_manager: SessionManager,
    store: Store,
    event_tx: Option<broadcast::Sender<PulpoEvent>>,
    node_name: String,
    interval: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(interval);
    tick.tick().await; // first tick completes immediately

    loop {
        tokio::select! {
            _ = tick.tick() => {
                check_and_run_schedules(&session_manager, &store, event_tx.as_ref(), &node_name)
                    .await
                    .unwrap_or_else(|e| warn!("Scheduler tick error: {e}"));
            }
            _ = shutdown_rx.changed() => {
                info!("Scheduler shutting down");
                break;
            }
        }
    }
}

/// Called each tick: loads active schedules, fires due ones.
async fn check_and_run_schedules(
    session_manager: &SessionManager,
    store: &Store,
    event_tx: Option<&broadcast::Sender<PulpoEvent>>,
    node_name: &str,
) -> Result<()> {
    let schedules = store.list_active_schedules().await?;
    let now = Utc::now();

    for schedule in schedules {
        let Some(next_run) = schedule.next_run_at else {
            continue;
        };

        if next_run > now {
            continue;
        }

        debug!(
            name = %schedule.name,
            next_run = %next_run,
            "Schedule is due"
        );

        fire_schedule(schedule, session_manager, store, event_tx, node_name).await;
    }

    Ok(())
}

/// Fire a single schedule: concurrency check, spawn, record execution, advance `next_run`.
async fn fire_schedule(
    mut schedule: Schedule,
    session_manager: &SessionManager,
    store: &Store,
    event_tx: Option<&broadcast::Sender<PulpoEvent>>,
    node_name: &str,
) {
    let run_number = schedule.execution_count + 1;

    // Concurrency check
    match evaluate_concurrency(&schedule, session_manager).await {
        Ok(true) => {} // proceed
        Ok(false) => {
            // Skip — record and advance
            let execution = ScheduleExecution {
                id: 0,
                schedule_id: schedule.id,
                session_id: None,
                status: ExecutionStatus::Skipped,
                error: None,
                triggered_by: "cron".into(),
                created_at: Utc::now(),
            };
            let _ = store.insert_execution(&execution).await;
            emit_schedule_event(event_tx, &schedule, "skipped", None, None, node_name);
            advance_schedule(&mut schedule, store).await;
            return;
        }
        Err(e) => {
            warn!(name = %schedule.name, "Concurrency check failed: {e}");
            record_failed_execution(&schedule, store, event_tx, node_name, &e.to_string()).await;
            advance_schedule(&mut schedule, store).await;
            return;
        }
    }

    // Build and spawn session
    let req = build_session_request(&schedule, run_number);
    match session_manager.create_session(req).await {
        Ok(session) => {
            let execution = ScheduleExecution {
                id: 0,
                schedule_id: schedule.id,
                session_id: Some(session.id),
                status: ExecutionStatus::Spawned,
                error: None,
                triggered_by: "cron".into(),
                created_at: Utc::now(),
            };
            let _ = store.insert_execution(&execution).await;

            schedule.last_session_id = Some(session.id);
            schedule.execution_count = run_number;
            schedule.last_run_at = Some(Utc::now());

            emit_schedule_event(
                event_tx,
                &schedule,
                "fired",
                Some(&session.id.to_string()),
                None,
                node_name,
            );

            // Check if max_executions reached
            if schedule
                .max_executions
                .is_some_and(|max| schedule.execution_count >= max)
            {
                schedule.status = ScheduleStatus::Exhausted;
                emit_schedule_event(event_tx, &schedule, "exhausted", None, None, node_name);
            }

            advance_schedule(&mut schedule, store).await;
        }
        Err(e) => {
            warn!(name = %schedule.name, "Failed to spawn session: {e}");
            record_failed_execution(&schedule, store, event_tx, node_name, &e.to_string()).await;
            advance_schedule(&mut schedule, store).await;
        }
    }
}

/// Records a failed execution and emits event.
async fn record_failed_execution(
    schedule: &Schedule,
    store: &Store,
    event_tx: Option<&broadcast::Sender<PulpoEvent>>,
    node_name: &str,
    error: &str,
) {
    let execution = ScheduleExecution {
        id: 0,
        schedule_id: schedule.id,
        session_id: None,
        status: ExecutionStatus::Failed,
        error: Some(error.to_owned()),
        triggered_by: "cron".into(),
        created_at: Utc::now(),
    };
    let _ = store.insert_execution(&execution).await;
    emit_schedule_event(event_tx, schedule, "failed", None, Some(error), node_name);
}

/// Advance `next_run_at` and persist the schedule.
async fn advance_schedule(schedule: &mut Schedule, store: &Store) {
    let now = Utc::now();
    match compute_next_run(&schedule.cron, now) {
        Ok(next) => schedule.next_run_at = Some(next),
        Err(e) => {
            warn!(name = %schedule.name, "Failed to compute next run: {e}");
            schedule.next_run_at = None;
        }
    }
    schedule.updated_at = Utc::now();
    if let Err(e) = store.update_schedule(schedule).await {
        warn!(name = %schedule.name, "Failed to update schedule: {e}");
    }
}

/// Emit a schedule event on the broadcast channel.
fn emit_schedule_event(
    event_tx: Option<&broadcast::Sender<PulpoEvent>>,
    schedule: &Schedule,
    event_type: &str,
    session_id: Option<&str>,
    error: Option<&str>,
    node_name: &str,
) {
    if let Some(tx) = event_tx {
        let event = ScheduleEvent {
            schedule_id: schedule.id.to_string(),
            schedule_name: schedule.name.clone(),
            event_type: event_type.to_owned(),
            session_id: session_id.map(ToOwned::to_owned),
            error: error.map(ToOwned::to_owned),
            node_name: node_name.to_owned(),
            timestamp: Utc::now().to_rfc3339(),
        };
        let _ = tx.send(PulpoEvent::Schedule(event));
    }
}

/// Manually trigger a schedule (ignores cron timing, respects concurrency).
pub async fn manual_run(
    schedule: &mut Schedule,
    session_manager: &SessionManager,
    store: &Store,
    event_tx: Option<&broadcast::Sender<PulpoEvent>>,
    node_name: &str,
) -> Result<Option<Uuid>> {
    if schedule.status == ScheduleStatus::Exhausted {
        bail!("schedule is exhausted (max_executions reached)");
    }

    let run_number = schedule.execution_count + 1;

    // Concurrency check
    let should_fire = evaluate_concurrency(schedule, session_manager).await?;
    if !should_fire {
        let execution = ScheduleExecution {
            id: 0,
            schedule_id: schedule.id,
            session_id: None,
            status: ExecutionStatus::Skipped,
            error: None,
            triggered_by: "manual".into(),
            created_at: Utc::now(),
        };
        store.insert_execution(&execution).await?;
        emit_schedule_event(event_tx, schedule, "skipped", None, None, node_name);
        return Ok(None);
    }

    let req = build_session_request(schedule, run_number);
    let session = session_manager.create_session(req).await?;

    let execution = ScheduleExecution {
        id: 0,
        schedule_id: schedule.id,
        session_id: Some(session.id),
        status: ExecutionStatus::Spawned,
        error: None,
        triggered_by: "manual".into(),
        created_at: Utc::now(),
    };
    store.insert_execution(&execution).await?;

    schedule.last_session_id = Some(session.id);
    schedule.execution_count = run_number;
    schedule.last_run_at = Some(Utc::now());

    emit_schedule_event(
        event_tx,
        schedule,
        "fired",
        Some(&session.id.to_string()),
        None,
        node_name,
    );

    // Check if max_executions reached
    if schedule
        .max_executions
        .is_some_and(|max| schedule.execution_count >= max)
    {
        schedule.status = ScheduleStatus::Exhausted;
        emit_schedule_event(event_tx, schedule, "exhausted", None, None, node_name);
    }

    // Recompute next_run_at
    match compute_next_run(&schedule.cron, Utc::now()) {
        Ok(next) => schedule.next_run_at = Some(next),
        Err(_) => schedule.next_run_at = None,
    }
    schedule.updated_at = Utc::now();
    store.update_schedule(schedule).await?;

    Ok(Some(session.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, TimeZone};
    use pulpo_common::schedule::ConcurrencyPolicy;
    use pulpo_common::session::{Provider, SessionMode};

    // --- validate_cron tests ---

    #[test]
    fn test_validate_cron_valid() {
        assert!(validate_cron("0 2 * * *").is_ok());
        assert!(validate_cron("*/5 * * * *").is_ok());
        assert!(validate_cron("0 0 1 * *").is_ok());
        assert!(validate_cron("* * * * *").is_ok());
        assert!(validate_cron("30 4 1,15 * *").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        let result = validate_cron("not a cron");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid cron"));
    }

    #[test]
    fn test_validate_cron_empty() {
        assert!(validate_cron("").is_err());
    }

    #[test]
    fn test_validate_cron_too_many_fields() {
        // 6 fields — after prepend/append becomes 8 which is invalid
        assert!(validate_cron("0 0 2 * * MON").is_err());
    }

    // --- to_seven_field tests ---

    #[test]
    fn test_to_seven_field() {
        assert_eq!(to_seven_field("0 2 * * *"), "0 0 2 * * * *");
        assert_eq!(to_seven_field("*/5 * * * *"), "0 */5 * * * * *");
    }

    // --- compute_next_run tests ---

    #[test]
    fn test_compute_next_run_basic() {
        let after = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let next = compute_next_run("0 2 * * *", after).unwrap();
        assert_eq!(next.hour(), 2);
        assert!(next > after);
    }

    #[test]
    fn test_compute_next_run_every_minute() {
        let after = Utc.with_ymd_and_hms(2026, 6, 15, 12, 30, 0).unwrap();
        let next = compute_next_run("* * * * *", after).unwrap();
        // Should be within 1 minute
        assert!(next - after <= ChronoDuration::minutes(1));
    }

    #[test]
    fn test_compute_next_run_invalid_cron() {
        let after = Utc::now();
        let result = compute_next_run("invalid", after);
        assert!(result.is_err());
    }

    // --- expand_template tests ---

    #[test]
    fn test_expand_template_all_vars() {
        let result = expand_template(
            "Review {schedule} run {run} on {date}",
            "nightly-review",
            42,
        );
        assert!(result.contains("nightly-review"));
        assert!(result.contains("42"));
        // Date should be YYYY-MM-DD format
        assert!(result.contains("2026-"));
    }

    #[test]
    fn test_expand_template_datetime() {
        let result = expand_template("Triggered at {datetime}", "test", 1);
        // Should contain RFC3339 datetime
        assert!(result.contains('T'));
        assert!(result.contains("+00:00") || result.contains("UTC"));
    }

    #[test]
    fn test_expand_template_no_vars() {
        let result = expand_template("plain prompt with no variables", "test", 1);
        assert_eq!(result, "plain prompt with no variables");
    }

    #[test]
    fn test_expand_template_empty() {
        let result = expand_template("", "test", 1);
        assert_eq!(result, "");
    }

    #[test]
    fn test_expand_template_multiple_same_var() {
        let result = expand_template("{run} and {run} again", "sched", 5);
        assert_eq!(result, "5 and 5 again");
    }

    // --- build_session_request tests ---

    fn make_schedule() -> Schedule {
        Schedule {
            id: Uuid::new_v4(),
            name: "nightly-review".into(),
            cron: "0 2 * * *".into(),
            workdir: "/tmp/repo".into(),
            prompt: "Review code for {schedule} run {run}".into(),
            provider: Provider::Claude,
            mode: SessionMode::Autonomous,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            concurrency: ConcurrencyPolicy::Skip,
            status: ScheduleStatus::Active,
            max_executions: Some(100),
            execution_count: 41,
            last_run_at: None,
            next_run_at: None,
            last_session_id: None,
            worktree: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_build_session_request_basic() {
        let schedule = make_schedule();
        let req = build_session_request(&schedule, 42);

        assert_eq!(req.name, Some("nightly-review-42".into()));
        assert_eq!(req.workdir, "/tmp/repo");
        assert_eq!(req.provider, Some(Provider::Claude));
        assert_eq!(req.mode, Some(SessionMode::Autonomous));
        assert!(req.prompt.contains("nightly-review"));
        assert!(req.prompt.contains("42"));
        assert!(req.model.is_none());
        assert!(req.allowed_tools.is_none());
        assert!(req.system_prompt.is_none());
        assert!(req.persona.is_none());
        assert!(req.guard_preset.is_none());
    }

    #[test]
    fn test_build_session_request_with_optionals() {
        let mut schedule = make_schedule();
        schedule.guard_preset = Some("standard".into());
        schedule.model = Some("opus".into());
        schedule.allowed_tools = Some(vec!["Read".into(), "Grep".into()]);
        schedule.system_prompt = Some("Be thorough".into());
        schedule.persona = Some("reviewer".into());

        let req = build_session_request(&schedule, 1);

        assert_eq!(req.model, Some("opus".into()));
        assert_eq!(req.allowed_tools, Some(vec!["Read".into(), "Grep".into()]));
        assert_eq!(req.system_prompt, Some("Be thorough".into()));
        assert_eq!(req.persona, Some("reviewer".into()));
        assert_eq!(
            req.guard_preset,
            Some(pulpo_common::guard::GuardPreset::Standard)
        );
    }

    #[test]
    fn test_build_session_request_minimal() {
        let schedule = Schedule {
            id: Uuid::new_v4(),
            name: "simple".into(),
            cron: "* * * * *".into(),
            workdir: "/tmp".into(),
            prompt: "do thing".into(),
            provider: Provider::Codex,
            mode: SessionMode::Interactive,
            guard_preset: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            concurrency: ConcurrencyPolicy::Allow,
            status: ScheduleStatus::Active,
            max_executions: None,
            execution_count: 0,
            last_run_at: None,
            next_run_at: None,
            last_session_id: None,
            worktree: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let req = build_session_request(&schedule, 1);

        assert_eq!(req.name, Some("simple-1".into()));
        assert_eq!(req.prompt, "do thing");
        assert_eq!(req.provider, Some(Provider::Codex));
        assert!(req.guard_preset.is_none());
        assert!(req.guard_config.is_none());
        assert!(req.model.is_none());
    }

    // --- evaluate_concurrency tests ---

    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::backend::Backend;

    struct MockBackend {
        alive: bool,
    }

    impl Backend for MockBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> anyhow::Result<bool> {
            Ok(self.alive)
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

    #[test]
    fn test_mock_backend_all_methods() {
        let b = MockBackend { alive: true };
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    /// Backend that fails on `create_session` — for testing spawn failure paths.
    struct FailingBackend;

    impl Backend for FailingBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("backend create failed")
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

    #[test]
    fn test_failing_backend_all_methods() {
        let b = FailingBackend;
        assert!(b.create_session("n", "d", "c").is_err());
        assert!(b.kill_session("n").is_ok());
        assert!(!b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    fn test_failing_session_manager(store: &Store) -> SessionManager {
        let backend: Arc<dyn Backend> = Arc::new(FailingBackend);
        SessionManager::new(
            backend,
            store.clone(),
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
    }

    async fn test_store() -> Store {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        store
    }

    fn test_session_manager(store: &Store, alive: bool) -> SessionManager {
        let backend: Arc<dyn Backend> = Arc::new(MockBackend { alive });
        SessionManager::new(
            backend,
            store.clone(),
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        )
    }

    #[tokio::test]
    async fn test_evaluate_concurrency_no_last_session() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);
        let schedule = make_schedule();

        let result = evaluate_concurrency(&schedule, &manager).await.unwrap();
        assert!(result); // No prior session → always fire
    }

    #[tokio::test]
    async fn test_evaluate_concurrency_skip_running() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        // Create a running session
        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "test-session".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-test-session".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.concurrency = ConcurrencyPolicy::Skip;

        let result = evaluate_concurrency(&schedule, &manager).await.unwrap();
        assert!(!result); // Should skip
    }

    #[tokio::test]
    async fn test_evaluate_concurrency_allow_running() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "test-session-allow".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-test-session-allow".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.concurrency = ConcurrencyPolicy::Allow;

        let result = evaluate_concurrency(&schedule, &manager).await.unwrap();
        assert!(result); // Should allow
    }

    #[tokio::test]
    async fn test_evaluate_concurrency_replace_running() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "test-session-replace".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-test-session-replace".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.concurrency = ConcurrencyPolicy::Replace;

        let result = evaluate_concurrency(&schedule, &manager).await.unwrap();
        assert!(result); // Should replace (kill + fire)

        // Verify session was killed
        let updated = store.get_session(&session.id.to_string()).await.unwrap();
        assert!(updated.is_some());
        // Session status should be dead after kill
        assert_eq!(updated.unwrap().status, SessionStatus::Dead);
    }

    #[tokio::test]
    async fn test_evaluate_concurrency_dead_session() {
        let store = test_store().await;
        let manager = test_session_manager(&store, false); // not alive

        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "test-dead".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Dead,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-test-dead".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.concurrency = ConcurrencyPolicy::Skip;

        let result = evaluate_concurrency(&schedule, &manager).await.unwrap();
        assert!(result); // Dead session — always fire regardless of policy
    }

    #[tokio::test]
    async fn test_evaluate_concurrency_missing_session() {
        let store = test_store().await;
        let manager = test_session_manager(&store, false);

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(Uuid::new_v4()); // Non-existent session

        let result = evaluate_concurrency(&schedule, &manager).await.unwrap();
        assert!(result); // Missing session → fire
    }

    // --- emit_schedule_event tests ---

    #[test]
    fn test_emit_schedule_event_with_tx() {
        let (tx, mut rx) = broadcast::channel(16);
        let schedule = make_schedule();

        emit_schedule_event(
            Some(&tx),
            &schedule,
            "fired",
            Some("session-123"),
            None,
            "node-1",
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(&event, PulpoEvent::Schedule(se) if
            se.event_type == "fired" &&
            se.session_id == Some("session-123".into()) &&
            se.error.is_none() &&
            se.node_name == "node-1" &&
            se.schedule_name == "nightly-review"
        ));
    }

    #[test]
    fn test_emit_schedule_event_with_error() {
        let (tx, mut rx) = broadcast::channel(16);
        let schedule = make_schedule();

        emit_schedule_event(
            Some(&tx),
            &schedule,
            "failed",
            None,
            Some("spawn error"),
            "node-1",
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(&event, PulpoEvent::Schedule(se) if
            se.event_type == "failed" &&
            se.error == Some("spawn error".into())
        ));
    }

    #[test]
    fn test_emit_schedule_event_no_tx() {
        let schedule = make_schedule();
        // Should not panic with None event_tx
        emit_schedule_event(None, &schedule, "fired", None, None, "node-1");
    }

    // --- run_scheduler_loop tests ---

    #[tokio::test]
    async fn test_scheduler_loop_shutdown() {
        let store = test_store().await;
        let manager = test_session_manager(&store, false);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handle = tokio::spawn(run_scheduler_loop(
            manager,
            store,
            None,
            "test-node".into(),
            Duration::from_millis(10),
            shutdown_rx,
        ));

        // Signal shutdown immediately
        shutdown_tx.send(true).unwrap();

        // Loop should exit quickly
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
    }

    // --- check_and_run_schedules tests ---

    #[tokio::test]
    async fn test_check_and_run_no_schedules() {
        let store = test_store().await;
        let manager = test_session_manager(&store, false);

        let result = check_and_run_schedules(&manager, &store, None, "node").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_and_run_not_due() {
        let store = test_store().await;
        let manager = test_session_manager(&store, false);

        let mut schedule = make_schedule();
        // Set next_run_at far in the future
        schedule.next_run_at = Some(Utc::now() + ChronoDuration::hours(24));
        store.insert_schedule(&schedule).await.unwrap();

        let result = check_and_run_schedules(&manager, &store, None, "node").await;
        assert!(result.is_ok());

        // Schedule should not have fired — execution_count unchanged
        let updated = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.execution_count, schedule.execution_count);
    }

    #[tokio::test]
    async fn test_check_and_run_due_schedule() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true); // alive backend for create_session

        let mut schedule = make_schedule();
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(5)); // overdue
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, mut rx) = broadcast::channel(16);
        let result = check_and_run_schedules(&manager, &store, Some(&tx), "node").await;
        assert!(result.is_ok());

        // Schedule should have fired — check event was emitted
        let event = rx.try_recv().unwrap();
        // First event should be the session creation event (from SessionManager)
        // or the schedule "fired" event
        match event {
            PulpoEvent::Session(_) | PulpoEvent::Schedule(_) => {} // both OK
        }

        // Execution count should have incremented
        let updated = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.execution_count, schedule.execution_count + 1);
        assert!(updated.last_run_at.is_some());
        assert!(updated.last_session_id.is_some());
    }

    #[tokio::test]
    async fn test_check_and_run_schedule_no_next_run() {
        let store = test_store().await;
        let manager = test_session_manager(&store, false);

        let mut schedule = make_schedule();
        schedule.next_run_at = None; // No next_run_at
        store.insert_schedule(&schedule).await.unwrap();

        let result = check_and_run_schedules(&manager, &store, None, "node").await;
        assert!(result.is_ok());

        // Should not have fired
        let updated = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.execution_count, schedule.execution_count);
    }

    // --- manual_run tests ---

    #[tokio::test]
    async fn test_manual_run_success() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        let mut schedule = make_schedule();
        schedule.next_run_at = Some(Utc::now() + ChronoDuration::hours(24));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, _rx) = broadcast::channel(16);
        let result = manual_run(&mut schedule, &manager, &store, Some(&tx), "node").await;
        let session_id = result.expect("manual_run should succeed");
        assert!(session_id.is_some());

        // Verify execution was recorded
        let execs = store
            .list_executions(&schedule.id.to_string(), 10)
            .await
            .unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].triggered_by, "manual");
        assert_eq!(execs[0].status, ExecutionStatus::Spawned);
    }

    #[tokio::test]
    async fn test_manual_run_exhausted() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        let mut schedule = make_schedule();
        schedule.status = ScheduleStatus::Exhausted;

        let result = manual_run(&mut schedule, &manager, &store, None, "node").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exhausted"));
    }

    #[tokio::test]
    async fn test_manual_run_skip_concurrency() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        // Create a running session
        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "running-session".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-running-session".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.concurrency = ConcurrencyPolicy::Skip;
        store.insert_schedule(&schedule).await.unwrap();

        let result = manual_run(&mut schedule, &manager, &store, None, "node").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // Skipped

        // Verify skipped execution was recorded
        let execs = store
            .list_executions(&schedule.id.to_string(), 10)
            .await
            .unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].status, ExecutionStatus::Skipped);
    }

    #[tokio::test]
    async fn test_manual_run_max_executions_reached() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        let mut schedule = make_schedule();
        schedule.max_executions = Some(42);
        schedule.execution_count = 41; // One more run will reach max
        schedule.next_run_at = Some(Utc::now() + ChronoDuration::hours(1));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, mut rx) = broadcast::channel(16);

        // Inject a session event to exercise the non-Schedule branch in the drain loop
        let _ = tx.send(PulpoEvent::Session(pulpo_common::event::SessionEvent {
            session_id: "dummy".into(),
            session_name: "dummy".into(),
            status: "running".into(),
            previous_status: None,
            node_name: "n".into(),
            output_snippet: None,
            waiting_for_input: None,
            timestamp: "t".into(),
        }));

        let result = manual_run(&mut schedule, &manager, &store, Some(&tx), "node").await;
        assert!(result.is_ok());

        // Should have spawned
        assert!(result.unwrap().is_some());

        // Check for exhausted event — drain events (includes the injected Session event)
        let mut found_exhausted = false;
        while let Ok(event) = rx.try_recv() {
            if let PulpoEvent::Schedule(se) = event
                && se.event_type == "exhausted"
            {
                found_exhausted = true;
            }
        }
        assert!(found_exhausted);

        // Schedule should be exhausted now
        let updated = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, ScheduleStatus::Exhausted);
    }

    // --- advance_schedule tests ---

    #[tokio::test]
    async fn test_advance_schedule_updates_next_run() {
        let store = test_store().await;

        let mut schedule = make_schedule();
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(5));
        store.insert_schedule(&schedule).await.unwrap();

        advance_schedule(&mut schedule, &store).await;

        assert!(schedule.next_run_at.is_some());
        assert!(schedule.next_run_at.unwrap() > Utc::now());
    }

    // --- fire_schedule tests ---

    #[tokio::test]
    async fn test_fire_schedule_spawn_failure() {
        // Use a backend where create_session fails (session name collision via store)
        let store = test_store().await;
        let manager = test_session_manager(&store, false); // will mark status=stale, but create succeeds

        let mut schedule = make_schedule();
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(1));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, _rx) = broadcast::channel(16);
        fire_schedule(schedule.clone(), &manager, &store, Some(&tx), "node").await;

        // Should have recorded an execution (spawned or failed depending on backend)
        let execs = store
            .list_executions(&schedule.id.to_string(), 10)
            .await
            .unwrap();
        assert!(!execs.is_empty());
    }

    #[tokio::test]
    async fn test_fire_schedule_concurrency_skip() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        // Create running session
        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "skip-test".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-skip-test".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.concurrency = ConcurrencyPolicy::Skip;
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(1));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, mut rx) = broadcast::channel(16);
        fire_schedule(schedule.clone(), &manager, &store, Some(&tx), "node").await;

        // Should have emitted a "skipped" event
        let event = rx.try_recv().unwrap();
        assert!(matches!(&event, PulpoEvent::Schedule(se) if se.event_type == "skipped"));

        // Execution should be recorded as skipped
        let execs = store
            .list_executions(&schedule.id.to_string(), 10)
            .await
            .unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].status, ExecutionStatus::Skipped);
    }

    use chrono::Timelike;

    #[tokio::test]
    async fn test_check_and_run_exhausted_after_fire() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        let mut schedule = make_schedule();
        schedule.max_executions = Some(42);
        schedule.execution_count = 41;
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(1));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, _rx) = broadcast::channel(16);
        check_and_run_schedules(&manager, &store, Some(&tx), "node")
            .await
            .unwrap();

        let updated = store
            .get_schedule(&schedule.id.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, ScheduleStatus::Exhausted);
        assert_eq!(updated.execution_count, 42);
    }

    // --- record_failed_execution tests ---

    #[tokio::test]
    async fn test_record_failed_execution() {
        let store = test_store().await;
        let schedule = make_schedule();
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, mut rx) = broadcast::channel(16);
        record_failed_execution(&schedule, &store, Some(&tx), "node", "boom").await;

        let execs = store
            .list_executions(&schedule.id.to_string(), 10)
            .await
            .unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].status, ExecutionStatus::Failed);
        assert_eq!(execs[0].error, Some("boom".into()));

        let event = rx.try_recv().unwrap();
        assert!(
            matches!(&event, PulpoEvent::Schedule(se) if se.event_type == "failed" && se.error == Some("boom".into()))
        );
    }

    // --- fire_schedule spawn failure tests ---

    #[tokio::test]
    async fn test_fire_schedule_backend_create_fails() {
        let store = test_store().await;
        let manager = test_failing_session_manager(&store);

        let mut schedule = make_schedule();
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(1));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, mut rx) = broadcast::channel(16);
        fire_schedule(schedule.clone(), &manager, &store, Some(&tx), "node").await;

        // Should have recorded a failed execution
        let execs = store
            .list_executions(&schedule.id.to_string(), 10)
            .await
            .unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].status, ExecutionStatus::Failed);
        assert!(execs[0].error.is_some());

        // Should have emitted "failed" event
        let event = rx.try_recv().unwrap();
        assert!(matches!(&event, PulpoEvent::Schedule(se) if se.event_type == "failed"));
    }

    // --- advance_schedule error path tests ---

    #[tokio::test]
    async fn test_advance_schedule_invalid_cron() {
        let store = test_store().await;

        let mut schedule = make_schedule();
        schedule.cron = "not-a-cron".into(); // Invalid cron after insertion
        store.insert_schedule(&schedule).await.unwrap();

        advance_schedule(&mut schedule, &store).await;

        // next_run_at should be None due to invalid cron
        assert!(schedule.next_run_at.is_none());
    }

    // --- check_and_run with spawn failure ---

    #[tokio::test]
    async fn test_check_and_run_spawn_failure() {
        let store = test_store().await;
        let manager = test_failing_session_manager(&store);

        let mut schedule = make_schedule();
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(1));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, _rx) = broadcast::channel(16);
        let result = check_and_run_schedules(&manager, &store, Some(&tx), "node").await;
        assert!(result.is_ok()); // check_and_run doesn't propagate fire errors

        // Should have recorded a failed execution
        let execs = store
            .list_executions(&schedule.id.to_string(), 10)
            .await
            .unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].status, ExecutionStatus::Failed);
    }

    // --- emit_schedule_event match arm coverage ---

    #[test]
    fn test_emit_schedule_event_with_all_fields() {
        let (tx, mut rx) = broadcast::channel(16);
        let schedule = make_schedule();

        emit_schedule_event(
            Some(&tx),
            &schedule,
            "exhausted",
            Some("sess-abc"),
            Some("max reached"),
            "test-node",
        );

        let event = rx.try_recv().unwrap();
        assert!(matches!(&event, PulpoEvent::Schedule(se) if
            se.event_type == "exhausted" &&
            se.session_id == Some("sess-abc".into()) &&
            se.error == Some("max reached".into())
        ));
    }

    // --- run_scheduler_loop tick error path ---

    #[tokio::test]
    async fn test_scheduler_loop_tick_error() {
        let store = test_store().await;
        let manager = test_session_manager(&store, false);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Close the DB pool so list_active_schedules() fails
        store.pool().close().await;

        let handle = tokio::spawn(run_scheduler_loop(
            manager,
            store,
            None,
            "test-node".into(),
            Duration::from_millis(10),
            shutdown_rx,
        ));

        // Let it tick once (with error), then shutdown
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
    }

    // --- fire_schedule concurrency check Err path ---

    /// Backend where `is_alive` fails — triggers concurrency check error.
    struct IsAliveFailingBackend;

    impl Backend for IsAliveFailingBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn is_alive(&self, _: &str) -> anyhow::Result<bool> {
            anyhow::bail!("is_alive failed")
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

    #[test]
    fn test_is_alive_failing_backend_all_methods() {
        let b = IsAliveFailingBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_ok());
        assert!(b.is_alive("n").is_err());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_fire_schedule_concurrency_check_error() {
        let store = test_store().await;
        let backend: Arc<dyn Backend> = Arc::new(IsAliveFailingBackend);
        let manager = SessionManager::new(
            backend,
            store.clone(),
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );

        // Create a running session so evaluate_concurrency calls is_alive
        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "conc-fail-session".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-conc-fail-session".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.next_run_at = Some(Utc::now() - ChronoDuration::minutes(5));
        store.insert_schedule(&schedule).await.unwrap();

        let (tx, mut rx) = broadcast::channel(16);

        // fire_schedule should hit the concurrency Err branch (lines 216-220)
        fire_schedule(schedule, &manager, &store, Some(&tx), "node").await;

        // Should have emitted a "failed" event
        let event = rx.try_recv().unwrap();
        assert!(matches!(&event, PulpoEvent::Schedule(se) if se.event_type == "failed"));

        // Should have recorded a failed execution — check via the store's schedule
        let all_scheds = store.list_schedules().await.unwrap();
        let s = all_scheds.first().unwrap();
        let execs = store.list_executions(&s.id.to_string(), 10).await.unwrap();
        assert_eq!(execs.len(), 1);
        assert_eq!(execs[0].status, ExecutionStatus::Failed);
    }

    // --- evaluate_concurrency Replace kill error ---

    /// Backend where `is_alive` returns true but `kill_session` fails.
    struct KillFailingBackend;

    impl Backend for KillFailingBackend {
        fn create_session(&self, _: &str, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn kill_session(&self, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("kill failed")
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

    #[test]
    fn test_kill_failing_backend_all_methods() {
        let b = KillFailingBackend;
        assert!(b.create_session("n", "d", "c").is_ok());
        assert!(b.kill_session("n").is_err());
        assert!(b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    #[tokio::test]
    async fn test_evaluate_concurrency_replace_kill_error() {
        let store = test_store().await;
        let backend: Arc<dyn Backend> = Arc::new(KillFailingBackend);
        let manager = SessionManager::new(
            backend,
            store.clone(),
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );

        let session = pulpo_common::session::Session {
            id: Uuid::new_v4(),
            name: "test-kill-fail".into(),
            workdir: "/tmp".into(),
            provider: Provider::Claude,
            prompt: "test".into(),
            status: SessionStatus::Running,
            mode: SessionMode::Interactive,
            conversation_id: None,
            exit_code: None,
            tmux_session: Some("pulpo-test-kill-fail".into()),
            output_snapshot: None,
            git_branch: None,
            git_sha: None,
            guard_config: None,
            model: None,
            allowed_tools: None,
            system_prompt: None,
            metadata: None,
            persona: None,
            max_turns: None,
            max_budget_usd: None,
            output_format: None,
            intervention_reason: None,
            intervention_at: None,
            recovery_count: 0,
            last_output_at: None,
            idle_since: None,
            waiting_for_input: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_session(&session).await.unwrap();

        let mut schedule = make_schedule();
        schedule.last_session_id = Some(session.id);
        schedule.concurrency = ConcurrencyPolicy::Replace;

        let result = evaluate_concurrency(&schedule, &manager).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("kill"));
    }

    // --- advance_schedule store update error ---

    #[tokio::test]
    async fn test_advance_schedule_store_update_error() {
        let store = test_store().await;
        let mut schedule = make_schedule();
        store.insert_schedule(&schedule).await.unwrap();

        // Close the pool so update_schedule fails
        store.pool().close().await;

        // advance_schedule should log a warning but not panic
        advance_schedule(&mut schedule, &store).await;
        // next_run_at should still be set (computed before store call)
        assert!(schedule.next_run_at.is_some());
    }

    // --- manual_run compute_next_run Err ---

    #[tokio::test]
    async fn test_manual_run_invalid_cron() {
        let store = test_store().await;
        let manager = test_session_manager(&store, true);

        let mut schedule = make_schedule();
        schedule.cron = "INVALID".into(); // Invalid cron — compute_next_run will fail
        // Insert with valid cron then update via raw SQL to bypass validation
        let original_cron = schedule.cron.clone();
        schedule.cron = "0 2 * * *".into();
        store.insert_schedule(&schedule).await.unwrap();

        // Update cron to invalid via raw SQL
        sqlx::query("UPDATE schedules SET cron = 'INVALID' WHERE id = ?")
            .bind(schedule.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        schedule.cron = original_cron;

        let (tx, _rx) = broadcast::channel(16);
        let result = manual_run(&mut schedule, &manager, &store, Some(&tx), "node").await;
        assert!(result.is_ok());
        // next_run_at should be None because invalid cron
        assert!(schedule.next_run_at.is_none());
    }
}
