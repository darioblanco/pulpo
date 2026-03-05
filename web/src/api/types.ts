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
  preset: string;
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
  persona: string | null;
  max_turns: number | null;
  max_budget_usd: number | null;
  output_format: string | null;
  intervention_reason: string | null;
  intervention_at: string | null;
  last_output_at: string | null;
  waiting_for_input: boolean;
  created_at: string;
}

export interface InterventionEvent {
  id: number;
  session_id: string;
  reason: string;
  created_at: string;
}

export interface PersonaConfig {
  provider: string | null;
  model: string | null;
  mode: string | null;
  guard_preset: string | null;
  allowed_tools: string[] | null;
  system_prompt: string | null;
  max_turns: number | null;
  max_budget_usd: number | null;
  output_format: string | null;
}

export interface PersonasResponse {
  personas: Record<string, PersonaConfig>;
}

export interface ListSessionsParams {
  status?: string;
  provider?: string;
  search?: string;
  sort?: string;
  order?: string;
}

export interface ConfigResponse {
  node: { name: string; port: number; data_dir: string };
  peers: Record<string, string>;
  guards: { preset: string };
}

export interface UpdateConfigRequest {
  node_name?: string;
  port?: number;
  data_dir?: string;
  guard_preset?: string;
  peers?: Record<string, string>;
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
  workdir: string;
  provider?: string;
  prompt: string;
  mode?: string;
  guard_preset?: string;
  model?: string;
  allowed_tools?: string[];
  system_prompt?: string;
  metadata?: Record<string, string>;
  persona?: string;
  max_turns?: number;
  max_budget_usd?: number;
  output_format?: string;
}
