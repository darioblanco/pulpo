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

export interface GuardConfig {
  unrestricted: boolean;
}

export interface Session {
  id: string;
  name: string;
  provider: string;
  status: string;
  prompt: string;
  mode: string;
  workdir: string;
  guard_config: GuardConfig | null;
  model: string | null;
  allowed_tools: string[] | null;
  system_prompt: string | null;
  metadata: Record<string, string> | null;
  ink: string | null;
  max_turns: number | null;
  max_budget_usd: number | null;
  output_format: string | null;
  intervention_reason: string | null;
  intervention_at: string | null;
  last_output_at: string | null;
  created_at: string;
}

export interface InterventionEvent {
  id: number;
  session_id: string;
  reason: string;
  created_at: string;
}

export interface Culture {
  id: string;
  session_id: string;
  kind: 'summary' | 'failure';
  scope_repo: string | null;
  scope_ink: string | null;
  title: string;
  body: string;
  tags: string[];
  relevance: number;
  created_at: string;
  last_referenced_at: string | null;
}

export interface CultureResponse {
  culture: Culture[];
}

export interface CultureItemResponse {
  culture: Culture;
}

export interface CultureDeleteResponse {
  deleted: boolean;
}

export interface CulturePushResponse {
  pushed: boolean;
  message: string;
}

export interface UpdateCultureRequest {
  title?: string;
  body?: string;
  tags?: string[];
  relevance?: number;
}

export interface CultureFileEntry {
  path: string;
  is_dir: boolean;
}

export interface CultureFilesResponse {
  files: CultureFileEntry[];
}

export interface CultureFileContentResponse {
  path: string;
  content: string;
}

export interface SyncStatus {
  enabled: boolean;
  last_sync: string | null;
  last_error: string | null;
  pending_commits: number;
  total_syncs: number;
}

export interface InkConfig {
  description: string | null;
  provider: string | null;
  model: string | null;
  mode: string | null;
  unrestricted: boolean | null;
  instructions: string | null;
}

export interface InksResponse {
  inks: Record<string, InkConfig>;
}

export interface ListSessionsParams {
  status?: string;
  provider?: string;
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
  default_provider: string | null;
}

export interface GuardDefaultConfigResponse {
  unrestricted: boolean;
}

export interface WatchdogConfigResponse {
  enabled: boolean;
  memory_threshold: number;
  check_interval_secs: number;
  breach_count: number;
  idle_timeout_secs: number;
  idle_action: string;
  finished_ttl_secs: number;
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
  guards: GuardDefaultConfigResponse;
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
  unrestricted?: boolean;
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
  name?: string;
  workdir?: string;
  provider?: string;
  prompt?: string;
  mode?: string;
  unrestricted?: boolean;
  model?: string;
  allowed_tools?: string[];
  system_prompt?: string;
  metadata?: Record<string, string>;
  ink?: string;
  max_turns?: number;
  max_budget_usd?: number;
  output_format?: string;
  worktree?: boolean;
  conversation_id?: string;
}

export interface CreateSessionResponse {
  session: Session;
  warnings?: string[];
}

/** Capabilities that a provider supports */
export interface ProviderCapabilities {
  model: boolean;
  system_prompt: boolean;
  allowed_tools: boolean;
  max_turns: boolean;
  max_budget_usd: boolean;
  output_format: boolean;
  worktree: boolean;
  unrestricted: boolean;
  resume: boolean;
}

/** Return the capability set for a given provider */
export function getProviderCapabilities(provider: string): ProviderCapabilities {
  switch (provider) {
    case 'claude':
      return {
        model: true,
        system_prompt: true,
        allowed_tools: true,
        max_turns: true,
        max_budget_usd: true,
        output_format: true,
        worktree: true,
        unrestricted: true,
        resume: true,
      };
    case 'codex':
      return {
        model: true,
        system_prompt: false,
        allowed_tools: false,
        max_turns: false,
        max_budget_usd: false,
        output_format: false,
        worktree: false,
        unrestricted: false,
        resume: true,
      };
    case 'gemini':
      return {
        model: true,
        system_prompt: false,
        allowed_tools: false,
        max_turns: false,
        max_budget_usd: false,
        output_format: true,
        worktree: false,
        unrestricted: true,
        resume: true,
      };
    case 'open_code':
    case 'opencode':
      return {
        model: false,
        system_prompt: false,
        allowed_tools: false,
        max_turns: false,
        max_budget_usd: false,
        output_format: true,
        worktree: false,
        unrestricted: false,
        resume: false,
      };
    default:
      // Unknown provider — assume full capabilities
      return {
        model: true,
        system_prompt: true,
        allowed_tools: true,
        max_turns: true,
        max_budget_usd: true,
        output_format: true,
        worktree: true,
        unrestricted: true,
        resume: true,
      };
  }
}
