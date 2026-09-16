export interface NodeInfo {
  name: string;
  hostname: string;
  os: string;
  arch: string;
  cpus: number;
  memory_mb: number;
  gpu: string | null;
}

/**
 * The five-state session model (ADR 0009): `starting` (spawn requested, backend not
 * yet confirmed), `working` (the agent process is running and busy), `waiting` (the
 * agent is at its prompt — see `Session.status_reason`), `done` (the process has
 * exited and the backend is gone — resumable), and `lost` (the backend died with no
 * evidence of a clean end — resumable). Mirrors `pulpo_common::session::SessionStatus`.
 * Replaces the old six-state model (`creating`, `active`, `idle`, `ready`, `stopped`,
 * `lost`) — `ready`/`stopped` merged into `done`, with the distinction moved to
 * `status_reason`. A live daemon only ever sends these five values.
 */
export type SessionStatus = 'starting' | 'working' | 'waiting' | 'done' | 'lost';

export interface Session {
  id: string;
  name: string;
  status: SessionStatus;
  /** Why the session is in `status` — see `pulpo_common::session::status_reason`.
   * Only meaningful for `waiting` (`"idle"` or `"needs_input:<reason>"`) and `done`
   * (`"exited"`, `"stopped"`, `"idle_timeout"`, `"budget_exceeded"`,
   * `"memory_pressure"`, or another/absent value treated as a generic fallback).
   * Always `null` for `starting`/`working`/`lost`. */
  status_reason: string | null;
  exit_code?: number | null;
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
  status: SessionStatus;
  output_snippet: string | null;
  /** Why the session is in `status` — see `Session.status_reason`. Omitted from the
   * wire payload (not just `null`) when not applicable (`starting`/`working`/`lost`).
   * The event is authoritative for a session already known client-side: always sync
   * (set-or-clear) the local value from this field rather than only ever setting it. */
  status_reason?: string | null;
  /** Deprecated for removal — the needs-input sub-reason (e.g. "permission"), kept
   * for one release after ADR 0009 for older consumers. Populated from
   * `status_reason` server-side; prefer reading `status_reason` directly (or
   * `needsInputReason()` in `@/lib/utils`) going forward. */
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
  idle_threshold_secs: number;
  extra_waiting_patterns: string[];
}

export interface WebhookEndpointConfigResponse {
  name: string;
  /** Masked server-side: scheme + host + first path segment only (e.g.
   * `https://hooks.slack.com/services/***`) — the delivery secret Slack/Discord/etc.
   * embed in the rest of the URL is never sent to this endpoint. */
  url: string;
  events: string[];
  min_severity?: string | null;
}

export interface NotificationsConfigResponse {
  webhooks: WebhookEndpointConfigResponse[];
}

/** Read-only view of pulpod's effective configuration — see `pulpo_common::api::ConfigResponse`.
 * The config file (`~/.pulpo/config.toml`) is the source of truth; the web UI only reads it. */
export interface ConfigResponse {
  node: NodeConfigResponse;
  watchdog: WatchdogConfigResponse;
  notifications: NotificationsConfigResponse;
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
