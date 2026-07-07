export interface NodeInfo {
  name: string;
  hostname: string;
  os: string;
  arch: string;
  cpus: number;
  memory_mb: number;
  gpu: string | null;
}

export interface PeerInfo {
  name: string;
  address: string;
  status: 'online' | 'offline' | 'unknown';
  node_info: NodeInfo | null;
  session_count: number | null;
  source?: 'configured' | 'discovered';
}

export interface PeersResponse {
  local: NodeInfo;
  peers: PeerInfo[];
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
  last_output_at: string | null;
  output_snippet?: string | null;
  created_at: string;
  updated_at?: string;
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
  tag: string | null;
  discovery_interval_secs: number;
}

export interface WatchdogConfigResponse {
  enabled: boolean;
  memory_threshold: number;
  check_interval_secs: number;
  breach_count: number;
  idle_timeout_secs: number;
  idle_action: string;
  ready_ttl_secs: number;
  adopt_tmux: boolean;
}

export interface WebhookEndpointConfigResponse {
  name: string;
  url: string;
  events: string[];
  has_secret: boolean;
}

export interface WebhookEndpointUpdateRequest {
  name: string;
  url: string;
  events: string[];
  secret?: string | null;
}

export interface NotificationsConfigResponse {
  webhooks: WebhookEndpointConfigResponse[];
}

export type PeerEntry = string | { address: string; token?: string };

export interface ConfigResponse {
  node: NodeConfigResponse;
  peers: Record<string, PeerEntry>;
  watchdog: WatchdogConfigResponse;
  notifications: NotificationsConfigResponse;
}

export interface UpdateConfigRequest {
  node_name?: string;
  port?: number;
  data_dir?: string;
  bind?: string;
  tag?: string;
  discovery_interval_secs?: number;
  watchdog_enabled?: boolean;
  watchdog_memory_threshold?: number;
  watchdog_check_interval_secs?: number;
  watchdog_breach_count?: number;
  watchdog_idle_timeout_secs?: number;
  watchdog_idle_action?: string;
  webhooks?: WebhookEndpointUpdateRequest[];
  peers?: Record<string, PeerEntry>;
}

export interface UpdateConfigResponse {
  config: ConfigResponse;
  restart_required: boolean;
}

export interface PairingUrlResponse {
  url: string;
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
  secrets?: string[];
  target_node?: string;
}

export interface CleanupSessionsResponse {
  deleted: number;
}

export interface CreateSessionResponse {
  session: Session;
}

export interface VapidPublicKeyResponse {
  public_key: string;
}

export interface PushSubscriptionRequest {
  endpoint: string;
  keys: {
    p256dh: string;
    auth: string;
  };
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

export interface SecretEntry {
  name: string;
  env?: string | null;
  created_at: string;
}

export interface SecretListResponse {
  secrets: SecretEntry[];
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

export interface SessionProjection {
  session_id: string;
  session_name: string;
  workdir: string;
  usage_source: string | null;
  auth_provider: string | null;
  auth_plan: string | null;
  auth_email: string | null;
  pool: string;
  total_tokens: number;
  cost_usd: number | null;
  elapsed_secs: number;
  cost_per_hour: number | null;
  tokens_per_hour: number | null;
  quota_used_percent: number | null;
  quota_resets_at: number | null;
  allowance_tokens: number | null;
  allowance_used_percent: number | null;
  secs_to_allowance: number | null;
}

export interface AccountRollup {
  provider: string | null;
  plan: string | null;
  email: string | null;
  pool: string;
  session_count: number;
  total_tokens: number;
  total_cost_usd: number | null;
  cost_per_hour: number | null;
  max_quota_used_percent: number | null;
  /** True when every cost-bearing session had an exact (structured-reader) source. */
  cost_is_exact: boolean;
}

/** Cost/token rollup for one attribution dimension value (a repo). */
export interface DimensionRollup {
  label: string;
  session_count: number;
  total_tokens: number;
  total_cost_usd: number | null;
  cost_per_hour: number | null;
  cost_is_exact: boolean;
}

export interface UsageProjectionResponse {
  node_name: string;
  generated_at: string;
  sessions: SessionProjection[];
  accounts: AccountRollup[];
  repos: DimensionRollup[];
}
