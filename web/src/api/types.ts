export interface NodeInfo {
  name: string;
  hostname: string;
  os: string;
  arch: string;
  cpus: number;
  memory_mb: number;
  gpu: string | null;
}

export interface Session {
  id: string;
  name: string;
  status: string;
  command: string;
  description: string | null;
  workdir: string;
  metadata: Record<string, string> | null;
  ink: string | null;
  intervention_reason: string | null;
  intervention_at: string | null;
  idle_threshold_secs?: number | null;
  worktree_path?: string | null;
  worktree_branch?: string | null;
  git_branch?: string | null;
  git_commit?: string | null;
  git_files_changed?: number | null;
  git_insertions?: number | null;
  git_deletions?: number | null;
  git_ahead?: number | null;
  /** 'docker' only appears on historical sessions — the docker runtime was removed. */
  runtime?: 'tmux' | 'docker';
  /** Harness adapter id (e.g. "claude"), set at spawn time when the command matched
   * a known harness. Absent for sessions spawned before harness adapters existed. */
  harness?: string | null;
  /** The harness's own session/thread id (e.g. Claude Code's `--session-id`). */
  harness_session_id?: string | null;
  /** When the harness last reported a lifecycle event. Presence means hook events
   * own this session's state (see docs/architecture/harness-adapters.md). */
  harness_last_event_at?: string | null;
  last_output_at: string | null;
  output_snippet?: string | null;
  created_at: string;
  updated_at?: string;
}

/** SSE `session` event payload — see `pulpo_common::event::SessionEvent`. */
export interface SessionSSEEvent {
  session_id: string;
  session_name: string;
  status: string;
  output_snippet: string | null;
  /** The `needs_input` metadata key (e.g. "permission"), when set. Absent — not
   * just an empty string — when the session has no `needs_input` metadata; the
   * event is authoritative, so a receiver should always sync (set-or-clear) its
   * local `metadata.needs_input` from this field rather than only setting it. */
  needs_input?: string | null;
}

export interface InterventionEvent {
  id: number;
  session_id: string;
  reason: string;
  created_at: string;
}

export interface ListSessionsParams {
  status?: string;
  search?: string;
  sort?: string;
  order?: string;
}

export interface NodeConfigResponse {
  name: string;
  port: number;
  data_dir: string;
  bind: string;
}

export interface WatchdogConfigResponse {
  enabled: boolean;
  check_interval_secs: number;
  idle_timeout_secs: number;
  idle_action: string;
}

export interface WebhookEndpointConfigResponse {
  name: string;
  url: string;
  events: string[];
}

export interface WebhookEndpointUpdateRequest {
  name: string;
  url: string;
  events: string[];
}

export interface NotificationsConfigResponse {
  webhooks: WebhookEndpointConfigResponse[];
}

export interface ConfigResponse {
  node: NodeConfigResponse;
  watchdog: WatchdogConfigResponse;
  notifications: NotificationsConfigResponse;
}

export interface UpdateConfigRequest {
  node_name?: string;
  port?: number;
  data_dir?: string;
  bind?: string;
  watchdog_enabled?: boolean;
  watchdog_check_interval_secs?: number;
  watchdog_idle_timeout_secs?: number;
  watchdog_idle_action?: string;
  webhooks?: WebhookEndpointUpdateRequest[];
}

export interface UpdateConfigResponse {
  config: ConfigResponse;
  restart_required: boolean;
}

export interface CreateSessionRequest {
  name: string;
  workdir?: string;
  command?: string;
  description?: string;
  metadata?: Record<string, string>;
  worktree?: boolean;
  worktree_base?: string;
  idle_threshold_secs?: number;
  target_node?: string;
}

export interface CleanupSessionsResponse {
  deleted: number;
}

export interface CreateSessionResponse {
  session: Session;
}

export interface ScheduleInfo {
  id: string;
  name: string;
  cron: string;
  command: string;
  workdir: string;
  target_node: string | null;
  /** Ink the schedule was created from (historical — the ink registry was removed). */
  ink: string | null;
  description: string | null;
  /** Cost budget in USD applied to every session this schedule fires. */
  budget_cost_usd?: number | null;
  enabled: boolean;
  last_run_at: string | null;
  last_session_id: string | null;
  last_attempted_at: string | null;
  last_error: string | null;
  created_at: string;
}

export interface CreateScheduleRequest {
  name: string;
  cron: string;
  command?: string;
  workdir: string;
  target_node?: string;
  description?: string;
  budget_cost_usd?: number;
}

/** Exact token/cost usage for one pulpo-managed session (see `pulpo_common::api::SessionUsage`). */
export interface SessionUsage {
  session_id: string;
  session_name: string;
  workdir: string;
  /** `claude-jsonl` / `codex-jsonl` / `pi-jsonl`, or `null` when the session has no
   * recorded usage yet (no structured reader matched, or nothing to read). */
  usage_source: string | null;
  total_tokens: number;
  cost_usd: number | null;
}

/** Cost/token rollup for one attribution dimension value (a repo). */
export interface DimensionRollup {
  label: string;
  session_count: number;
  total_tokens: number;
  total_cost_usd: number | null;
}

export interface UsageSessionsResponse {
  node_name: string;
  generated_at: string;
  sessions: SessionUsage[];
  repos: DimensionRollup[];
}
