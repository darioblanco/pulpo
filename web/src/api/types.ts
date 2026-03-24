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

export interface InkConfig {
  description: string | null;
  command: string | null;
}

export interface InksResponse {
  inks: Record<string, InkConfig>;
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
  seed: string | null;
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

export interface DiscordWebhookConfigResponse {
  webhook_url: string;
  events: string[];
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
  discord: DiscordWebhookConfigResponse | null;
  webhooks: WebhookEndpointConfigResponse[];
}

export type PeerEntry = string | { address: string; token?: string };

export interface ConfigResponse {
  node: NodeConfigResponse;
  peers: Record<string, PeerEntry>;
  watchdog: WatchdogConfigResponse;
  notifications: NotificationsConfigResponse;
  inks: Record<string, InkConfig>;
}

export interface UpdateConfigRequest {
  node_name?: string;
  port?: number;
  data_dir?: string;
  bind?: string;
  tag?: string;
  seed?: string;
  discovery_interval_secs?: number;
  watchdog_enabled?: boolean;
  watchdog_memory_threshold?: number;
  watchdog_check_interval_secs?: number;
  watchdog_breach_count?: number;
  watchdog_idle_timeout_secs?: number;
  watchdog_idle_action?: string;
  discord_webhook_url?: string;
  discord_events?: string[];
  webhooks?: WebhookEndpointUpdateRequest[];
  inks?: Record<string, InkConfig>;
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
  ink?: string;
  description?: string;
  metadata?: Record<string, string>;
  worktree?: boolean;
  worktree_base?: string;
  runtime?: 'tmux' | 'docker';
  secrets?: string[];
}

export interface CreateSessionResponse {
  session: Session;
}

export interface VapidPublicKeyResponse {
  public_key: string;
}

export interface FleetSession extends Session {
  node_name: string;
  node_address: string;
}

export interface FleetSessionsResponse {
  sessions: FleetSession[];
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
  ink: string | null;
  description: string | null;
  enabled: boolean;
  last_run_at: string | null;
  last_session_id: string | null;
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
  ink?: string;
  description?: string;
}
