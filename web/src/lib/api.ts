import { getBaseUrl, getAuthToken } from '$lib/stores/connection.svelte';

function resolveBaseUrl(): string {
  const base = getBaseUrl();
  return base ? `${base}/api/v1` : '/api/v1';
}

function authHeaders(extra?: Record<string, string>): Record<string, string> {
  const token = getAuthToken();
  const headers: Record<string, string> = { ...extra };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  return headers;
}

function authFetch(url: string, init?: RequestInit): Promise<Response> {
  const headers = authHeaders(init?.headers as Record<string, string>);
  return fetch(url, { ...init, headers });
}

export function resolveWsUrl(path: string): string {
  const base = getBaseUrl();
  const token = getAuthToken();
  let wsUrl: string;
  if (base) {
    const proto = base.startsWith('https') ? 'wss:' : 'ws:';
    const host = base.replace(/^https?:\/\//, '');
    wsUrl = `${proto}//${host}${path}`;
  } else {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    wsUrl = `${proto}//${location.host}${path}`;
  }
  if (token) {
    wsUrl += `?token=${encodeURIComponent(token)}`;
  }
  return wsUrl;
}

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
  recovery_count: number;
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
}

export interface PersonasResponse {
  personas: Record<string, PersonaConfig>;
}

export async function getPersonas(): Promise<PersonasResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/personas`);
  return res.json();
}

export async function getNode() {
  const res = await authFetch(`${resolveBaseUrl()}/node`);
  return res.json();
}

export async function getPeers(): Promise<PeersResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/peers`);
  return res.json();
}

export interface ListSessionsParams {
  status?: string;
  provider?: string;
  search?: string;
  sort?: string;
  order?: string;
}

export async function getSessions(params?: ListSessionsParams) {
  const base = resolveBaseUrl();
  const url = new URL(`${base}/sessions`, base.startsWith('http') ? base : window.location.origin);
  if (params) {
    for (const [key, value] of Object.entries(params)) {
      if (value !== undefined) url.searchParams.set(key, value);
    }
  }
  const fetchUrl = base.startsWith('http') ? url.toString() : url.pathname + url.search;
  const res = await authFetch(fetchUrl);
  return res.json();
}

export async function getRemoteSessions(address: string): Promise<Session[]> {
  const res = await authFetch(`http://${address}/api/v1/sessions`);
  return res.json();
}

export async function getSession(id: string) {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}`);
  return res.json();
}

export async function createSession(data: {
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
}) {
  const res = await authFetch(`${resolveBaseUrl()}/sessions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  return res.json();
}

export async function createRemoteSession(
  address: string,
  data: {
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
  },
) {
  const res = await authFetch(`http://${address}/api/v1/sessions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  return res.json();
}

export async function killSession(id: string) {
  await authFetch(`${resolveBaseUrl()}/sessions/${id}/kill`, { method: 'POST' });
}

export async function deleteSession(id: string) {
  await authFetch(`${resolveBaseUrl()}/sessions/${id}`, { method: 'DELETE' });
}

export async function getSessionOutput(id: string, lines: number = 100) {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}/output?lines=${lines}`);
  return res.json();
}

export async function sendInput(id: string, text: string) {
  await authFetch(`${resolveBaseUrl()}/sessions/${id}/input`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
}

export async function resumeSession(id: string) {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}/resume`, { method: 'POST' });
  return res.json();
}

export async function getInterventionEvents(id: string): Promise<InterventionEvent[]> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}/interventions`);
  return res.json();
}

export async function downloadSessionOutput(id: string): Promise<Blob> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}/output/download`);
  return res.blob();
}

export async function addPeer(name: string, address: string): Promise<PeersResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/peers`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, address }),
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to add peer');
  }
  return res.json();
}

export async function removePeer(name: string): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/peers/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to remove peer');
  }
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

export async function getConfig(): Promise<ConfigResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/config`);
  return res.json();
}

export interface PairingUrlResponse {
  url: string;
}

export async function getPairingUrl(): Promise<PairingUrlResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/auth/pairing-url`);
  return res.json();
}

export async function updateConfig(data: UpdateConfigRequest): Promise<UpdateConfigResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/config`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  return res.json();
}
