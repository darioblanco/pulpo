use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use pulpo_common::api::{
    CreateScheduleRequest, ErrorResponse, ListExecutionsQuery, UpdateScheduleRequest,
};
use pulpo_common::schedule::{Schedule, ScheduleExecution, ScheduleStatus};
use pulpo_common::session::Provider;
use uuid::Uuid;

use crate::schedule;

type ApiError = (StatusCode, Json<ErrorResponse>);

fn internal_error(msg: &str) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn not_found(msg: &str) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn bad_request(msg: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

fn conflict(msg: &str) -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(ErrorResponse {
            error: msg.to_owned(),
        }),
    )
}

pub async fn create(
    State(state): State<Arc<super::AppState>>,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<Schedule>), ApiError> {
    // Validate cron and compute first run time (compute_next_run validates as part of computing)
    let now = Utc::now();
    let next_run =
        schedule::compute_next_run(&req.cron, now).map_err(|e| bad_request(&e.to_string()))?;

    // Check for duplicate name
    let store = state.session_manager.store();
    let existing = store
        .get_schedule_by_name(&req.name)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    if existing.is_some() {
        return Err(conflict(&format!(
            "schedule with name '{}' already exists",
            req.name
        )));
    }

    let sched = Schedule {
        id: Uuid::new_v4(),
        name: req.name,
        cron: req.cron,
        workdir: req.workdir,
        prompt: req.prompt,
        provider: req.provider.unwrap_or(Provider::Claude),
        mode: req.mode.unwrap_or_default(),
        guard_preset: req.guard_preset,
        guard_config: req.guard_config,
        model: req.model,
        allowed_tools: req.allowed_tools,
        system_prompt: req.system_prompt,
        metadata: req.metadata,
        persona: req.persona,
        max_turns: req.max_turns,
        max_budget_usd: req.max_budget_usd,
        output_format: req.output_format.clone(),
        concurrency: req.concurrency.unwrap_or_default(),
        status: ScheduleStatus::Active,
        max_executions: req.max_executions,
        execution_count: 0,
        last_run_at: None,
        next_run_at: Some(next_run),
        last_session_id: None,
        worktree: req.worktree,
        created_at: now,
        updated_at: now,
    };

    store
        .insert_schedule(&sched)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok((StatusCode::CREATED, Json(sched)))
}

pub async fn list(
    State(state): State<Arc<super::AppState>>,
) -> Result<Json<Vec<Schedule>>, ApiError> {
    let schedules = state
        .session_manager
        .store()
        .list_schedules()
        .await
        .map_err(|e| internal_error(&e.to_string()))?;
    Ok(Json(schedules))
}

pub async fn get(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Schedule>, ApiError> {
    match state
        .session_manager
        .store()
        .get_schedule_by_id_or_name(&id)
        .await
    {
        Ok(Some(sched)) => Ok(Json(sched)),
        Ok(None) => Err(not_found(&format!("schedule not found: {id}"))),
        Err(e) => Err(internal_error(&e.to_string())),
    }
}

pub async fn update(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateScheduleRequest>,
) -> Result<Json<Schedule>, ApiError> {
    let store = state.session_manager.store();
    let mut sched = store
        .get_schedule_by_id_or_name(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| not_found(&format!("schedule not found: {id}")))?;

    // Apply partial updates
    if let Some(cron) = req.cron {
        // compute_next_run validates the cron expression as part of computing
        let next = schedule::compute_next_run(&cron, Utc::now())
            .map_err(|e| bad_request(&e.to_string()))?;
        sched.cron = cron;
        sched.next_run_at = Some(next);
    }
    if let Some(workdir) = req.workdir {
        sched.workdir = workdir;
    }
    if let Some(prompt) = req.prompt {
        sched.prompt = prompt;
    }
    if let Some(provider) = req.provider {
        sched.provider = provider;
    }
    if let Some(mode) = req.mode {
        sched.mode = mode;
    }
    if let Some(guard_preset) = req.guard_preset {
        sched.guard_preset = Some(guard_preset);
    }
    if let Some(guard_config) = req.guard_config {
        sched.guard_config = Some(guard_config);
    }
    if let Some(model) = req.model {
        sched.model = Some(model);
    }
    if let Some(allowed_tools) = req.allowed_tools {
        sched.allowed_tools = Some(allowed_tools);
    }
    if let Some(system_prompt) = req.system_prompt {
        sched.system_prompt = Some(system_prompt);
    }
    if let Some(metadata) = req.metadata {
        sched.metadata = Some(metadata);
    }
    if let Some(persona) = req.persona {
        sched.persona = Some(persona);
    }
    if let Some(max_turns) = req.max_turns {
        sched.max_turns = Some(max_turns);
    }
    if let Some(max_budget_usd) = req.max_budget_usd {
        sched.max_budget_usd = Some(max_budget_usd);
    }
    if let Some(output_format) = req.output_format {
        sched.output_format = Some(output_format);
    }
    if let Some(concurrency) = req.concurrency {
        sched.concurrency = concurrency;
    }
    if let Some(max_executions) = req.max_executions {
        sched.max_executions = Some(max_executions);
    }
    if let Some(worktree) = req.worktree {
        sched.worktree = Some(worktree);
    }

    sched.updated_at = Utc::now();
    store
        .update_schedule(&sched)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok(Json(sched))
}

pub async fn delete(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let store = state.session_manager.store();
    let sched = store
        .get_schedule_by_id_or_name(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| not_found(&format!("schedule not found: {id}")))?;

    store
        .delete_schedule(&sched.id.to_string())
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn run(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.session_manager.store();
    let mut sched = store
        .get_schedule_by_id_or_name(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| not_found(&format!("schedule not found: {id}")))?;

    let node_name = state.config.read().await.node.name.clone();
    let session_id = schedule::manual_run(
        &mut sched,
        &state.session_manager,
        store,
        Some(&state.event_tx),
        &node_name,
    )
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("exhausted") {
            bad_request(&msg)
        } else {
            internal_error(&msg)
        }
    })?;

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "status": if session_id.is_some() { "spawned" } else { "skipped" },
    })))
}

pub async fn pause(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Schedule>, ApiError> {
    let store = state.session_manager.store();
    let mut sched = store
        .get_schedule_by_id_or_name(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| not_found(&format!("schedule not found: {id}")))?;

    if sched.status != ScheduleStatus::Active {
        return Err(bad_request(&format!(
            "can only pause active schedules (current status: {})",
            sched.status
        )));
    }

    sched.status = ScheduleStatus::Paused;
    sched.updated_at = Utc::now();
    store
        .update_schedule(&sched)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok(Json(sched))
}

pub async fn resume(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Schedule>, ApiError> {
    let store = state.session_manager.store();
    let mut sched = store
        .get_schedule_by_id_or_name(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| not_found(&format!("schedule not found: {id}")))?;

    if sched.status != ScheduleStatus::Paused {
        return Err(bad_request(&format!(
            "can only resume paused schedules (current status: {})",
            sched.status
        )));
    }

    sched.status = ScheduleStatus::Active;
    // Recompute next_run_at from now (no catch-up)
    match schedule::compute_next_run(&sched.cron, Utc::now()) {
        Ok(next) => sched.next_run_at = Some(next),
        Err(_) => sched.next_run_at = None,
    }
    sched.updated_at = Utc::now();
    store
        .update_schedule(&sched)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok(Json(sched))
}

pub async fn executions(
    State(state): State<Arc<super::AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ListExecutionsQuery>,
) -> Result<Json<Vec<ScheduleExecution>>, ApiError> {
    let store = state.session_manager.store();
    let sched = store
        .get_schedule_by_id_or_name(&id)
        .await
        .map_err(|e| internal_error(&e.to_string()))?
        .ok_or_else(|| not_found(&format!("schedule not found: {id}")))?;

    let limit = query.limit.unwrap_or(20);
    let execs = store
        .list_executions(&sched.id.to_string(), limit)
        .await
        .map_err(|e| internal_error(&e.to_string()))?;

    Ok(Json(execs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::backend::Backend;
    use crate::config::{Config, NodeConfig};
    use crate::peers::PeerRegistry;
    use crate::session::manager::SessionManager;
    use crate::store::Store;
    use std::collections::HashMap;

    #[test]
    fn test_internal_error() {
        let (status, Json(body)) = internal_error("db down");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.error, "db down");
    }

    #[test]
    fn test_not_found() {
        let (status, Json(body)) = not_found("gone");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.error, "gone");
    }

    #[test]
    fn test_bad_request() {
        let (status, Json(body)) = bad_request("invalid");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "invalid");
    }

    #[test]
    fn test_conflict() {
        let (status, Json(body)) = conflict("exists");
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body.error, "exists");
    }

    // --- Direct handler error path tests ---

    struct FailingCreateBackend;

    impl Backend for FailingCreateBackend {
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
    fn test_failing_create_backend_methods() {
        let b = FailingCreateBackend;
        assert!(b.create_session("n", "d", "c").is_err());
        assert!(b.kill_session("n").is_ok());
        assert!(!b.is_alive("n").unwrap());
        assert!(b.capture_output("n", 10).unwrap().is_empty());
        assert!(b.send_input("n", "t").is_ok());
        assert!(b.setup_logging("n", "p").is_ok());
    }

    async fn failing_state() -> Arc<AppState> {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir = Box::leak(Box::new(tmpdir));
        let store = Store::new(tmpdir.path().to_str().unwrap()).await.unwrap();
        store.migrate().await.unwrap();
        let backend = Arc::new(FailingCreateBackend);
        let manager = SessionManager::new(
            backend,
            store,
            pulpo_common::guard::GuardConfig::default(),
            HashMap::new(),
        );
        let peer_registry = PeerRegistry::new(&HashMap::new());
        AppState::new(
            Config {
                node: NodeConfig {
                    name: "test-node".into(),
                    port: 7433,
                    data_dir: tmpdir.path().to_str().unwrap().into(),
                },
                auth: crate::config::AuthConfig::default(),
                peers: HashMap::new(),
                guards: crate::config::GuardDefaultConfig::default(),
                watchdog: crate::config::WatchdogConfig::default(),
                personas: HashMap::new(),
                notifications: crate::config::NotificationsConfig::default(),
            },
            manager,
            peer_registry,
        )
    }

    #[tokio::test]
    async fn test_get_corrupted_schedule() {
        let state = failing_state().await;
        let store = state.session_manager.store();

        // Insert a schedule with invalid provider via raw SQL
        sqlx::query(
            "INSERT INTO schedules (id, name, cron, workdir, prompt, provider, mode, concurrency, status, execution_count, created_at, updated_at)
             VALUES ('00000000-0000-4000-8000-000000000001', 'bad-sched', '0 0 * * *', '/tmp', 'test', 'INVALID_PROVIDER', 'interactive', 'skip', 'active', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(store.pool())
        .await
        .unwrap();

        let result = get(State(state), Path("bad-sched".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_run_spawn_failure() {
        let state = failing_state().await;
        let store = state.session_manager.store();

        // Create a valid schedule
        let now = chrono::Utc::now();
        let sched = Schedule {
            id: uuid::Uuid::new_v4(),
            name: "run-fail".into(),
            cron: "0 2 * * *".into(),
            workdir: "/tmp".into(),
            prompt: "test".into(),
            provider: Provider::Claude,
            mode: pulpo_common::session::SessionMode::default(),
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
            concurrency: pulpo_common::schedule::ConcurrencyPolicy::default(),
            status: ScheduleStatus::Active,
            max_executions: None,
            execution_count: 0,
            last_run_at: None,
            next_run_at: Some(now),
            last_session_id: None,
            worktree: None,
            created_at: now,
            updated_at: now,
        };
        store.insert_schedule(&sched).await.unwrap();

        // Run should fail because create_session fails → internal_error (not "exhausted")
        let result = run(State(state), Path("run-fail".into())).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_resume_next_run_recompute() {
        let state = failing_state().await;
        let store = state.session_manager.store();

        // Insert a paused schedule with invalid cron via raw SQL (bypasses validation)
        sqlx::query(
            "INSERT INTO schedules (id, name, cron, workdir, prompt, provider, mode, concurrency, status, execution_count, created_at, updated_at)
             VALUES ('00000000-0000-4000-8000-000000000002', 'bad-cron-resume', 'INVALID', '/tmp', 'test', 'claude', 'interactive', 'skip', 'paused', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(store.pool())
        .await
        .unwrap();

        let result = resume(State(state), Path("bad-cron-resume".into())).await;
        assert!(result.is_ok());
        let Json(sched) = result.unwrap();
        // next_run_at should be None because invalid cron can't compute next run
        assert!(sched.next_run_at.is_none());
    }

    #[tokio::test]
    async fn test_update_cron_recompute_err() {
        let state = failing_state().await;
        let store = state.session_manager.store();

        // Insert a schedule with invalid cron stored (bypasses validation)
        sqlx::query(
            "INSERT INTO schedules (id, name, cron, workdir, prompt, provider, mode, concurrency, status, execution_count, created_at, updated_at)
             VALUES ('00000000-0000-4000-8000-000000000003', 'bad-cron-update', '0 2 * * *', '/tmp', 'test', 'claude', 'interactive', 'skip', 'active', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(store.pool())
        .await
        .unwrap();

        // Update to a cron expression that validates but then we need compute_next_run to fail.
        // Since validation and computation use the same logic, we need a different approach.
        // Instead, directly mutate the schedule's cron to something invalid AFTER the validation
        // step in the handler. We can't do this through the API, so let's test the Err(_) branch
        // by providing a valid cron update which goes through the Ok path (covering line 159).
        // Line 160 (Err branch) requires a cron that validates but has no next occurrence -
        // which is impossible for standard 5-field cron. This is a defensive branch.
        // We cover lines 155-159 through this test.
        let req = pulpo_common::api::UpdateScheduleRequest {
            cron: Some("30 3 * * *".into()),
            workdir: None,
            prompt: None,
            provider: None,
            mode: None,
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
            concurrency: None,
            max_executions: None,
            worktree: None,
        };
        let result = update(State(state), Path("bad-cron-update".into()), Json(req)).await;
        assert!(result.is_ok());
    }

    // --- Store error tests (closed pool → triggers map_err closures) ---

    async fn closed_pool_state() -> Arc<AppState> {
        let state = failing_state().await;
        state.session_manager.store().pool().close().await;
        state
    }

    fn valid_create_req() -> CreateScheduleRequest {
        CreateScheduleRequest {
            name: "test-create".into(),
            cron: "0 2 * * *".into(),
            workdir: "/tmp".into(),
            prompt: "test".into(),
            provider: None,
            mode: None,
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
            concurrency: None,
            max_executions: None,
            worktree: None,
        }
    }

    fn empty_update_req() -> UpdateScheduleRequest {
        UpdateScheduleRequest {
            cron: None,
            workdir: None,
            prompt: None,
            provider: None,
            mode: None,
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
            concurrency: None,
            max_executions: None,
            worktree: None,
        }
    }

    #[tokio::test]
    async fn test_create_store_error() {
        let state = closed_pool_state().await;
        let result = create(State(state), Json(valid_create_req())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_list_store_error() {
        let state = closed_pool_state().await;
        let result = list(State(state)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_update_store_error() {
        let state = closed_pool_state().await;
        let result = update(State(state), Path("x".into()), Json(empty_update_req())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_delete_store_error() {
        let state = closed_pool_state().await;
        let result = delete(State(state), Path("x".into())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_run_store_error() {
        let state = closed_pool_state().await;
        let result = run(State(state), Path("x".into())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_pause_store_error() {
        let state = closed_pool_state().await;
        let result = pause(State(state), Path("x".into())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_resume_store_error() {
        let state = closed_pool_state().await;
        let result = resume(State(state), Path("x".into())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_executions_store_error() {
        let state = closed_pool_state().await;
        let result = executions(
            State(state),
            Path("x".into()),
            Query(ListExecutionsQuery { limit: None }),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_create_insert_store_error() {
        // Line 108: insert_schedule fails. Drop the schedules table so the insert fails
        // but the name check query also fails — can't easily separate them.
        // Instead, drop only the schedules table so get_schedule_by_name fails at the
        // SELECT query level. This triggers line 66 (already covered by test_create_store_error).
        // For line 108 specifically: we need get_schedule_by_name → Ok(None) then insert → Err.
        // Use a constraint violation by inserting a schedule with NOT NULL violation.
        // Actually the simplest: create a view with the same name as the table after dropping it.
        let state = failing_state().await;
        let store = state.session_manager.store();
        // Drop the table and recreate it without certain required columns
        sqlx::query("DROP TABLE schedules")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE schedules (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE
            )",
        )
        .execute(store.pool())
        .await
        .unwrap();

        let result = create(State(state), Json(valid_create_req())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// Break the schedules table schema so that `get_schedule_by_id_or_name` still works
    /// (reads existing rows) but `update_schedule` / `delete_schedule` fail.
    async fn break_schedule_writes(state: &AppState) {
        let pool = state.session_manager.store().pool();
        // Rename `updated_at` so UPDATE SET ... updated_at = ? fails
        sqlx::query("ALTER TABLE schedules RENAME COLUMN updated_at TO updated_at_old")
            .execute(pool)
            .await
            .unwrap();
    }

    /// Break the `schedule_executions` table so `list_executions` fails.
    async fn break_executions_table(state: &AppState) {
        let pool = state.session_manager.store().pool();
        sqlx::query("DROP TABLE schedule_executions")
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_update_save_store_error() {
        let state = failing_state().await;
        let req = valid_create_req();
        let (_, Json(sched)) = create(State(state.clone()), Json(req)).await.unwrap();

        break_schedule_writes(&state).await;
        let result = update(State(state), Path(sched.name), Json(empty_update_req())).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_delete_remove_store_error() {
        let state = failing_state().await;
        let req = valid_create_req();
        let (_, Json(sched)) = create(State(state.clone()), Json(req)).await.unwrap();

        // Create a trigger that prevents deletion
        let pool = state.session_manager.store().pool();
        sqlx::query(
            "CREATE TRIGGER no_delete BEFORE DELETE ON schedules
             BEGIN SELECT RAISE(ABORT, 'delete blocked by trigger'); END",
        )
        .execute(pool)
        .await
        .unwrap();

        let result = delete(State(state), Path(sched.name)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_pause_save_store_error() {
        let state = failing_state().await;
        let req = valid_create_req();
        let (_, Json(sched)) = create(State(state.clone()), Json(req)).await.unwrap();

        break_schedule_writes(&state).await;
        let result = pause(State(state), Path(sched.name)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_resume_save_store_error() {
        let state = failing_state().await;
        let req = valid_create_req();
        let (_, Json(sched)) = create(State(state.clone()), Json(req)).await.unwrap();
        let _ = pause(State(state.clone()), Path(sched.name.clone()))
            .await
            .unwrap();

        break_schedule_writes(&state).await;
        let result = resume(State(state), Path(sched.name)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_executions_list_store_error() {
        let state = failing_state().await;
        let req = valid_create_req();
        let (_, Json(sched)) = create(State(state.clone()), Json(req)).await.unwrap();

        break_executions_table(&state).await;
        let result = executions(
            State(state),
            Path(sched.name),
            Query(ListExecutionsQuery { limit: None }),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
