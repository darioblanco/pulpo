import type {
  Session,
  NodeInfo,
  PeersResponse,
  ListSessionsParams,
  InterventionEvent,
  ConfigResponse,
  UpdateConfigRequest,
  UpdateConfigResponse,
  PairingUrlResponse,
  InksResponse,
  CreateSessionRequest,
  CreateSessionResponse,
  KnowledgeResponse,
  KnowledgeItemResponse,
  KnowledgeDeleteResponse,
  KnowledgePushResponse,
  UpdateKnowledgeRequest,
} from './types';

let getBaseUrl: () => string = () => '';
let getAuthToken: () => string = () => '';

export function setApiConfig(config: { getBaseUrl: () => string; getAuthToken: () => string }) {
  getBaseUrl = config.getBaseUrl;
  getAuthToken = config.getAuthToken;
}

export function resolveBaseUrl(): string {
  const base = getBaseUrl();
  return base ? `${base}/api/v1` : '/api/v1';
}

export function authHeaders(extra?: Record<string, string>): Record<string, string> {
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

export async function getInks(): Promise<InksResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/inks`);
  return res.json();
}

export async function getNode(): Promise<NodeInfo> {
  const res = await authFetch(`${resolveBaseUrl()}/node`);
  return res.json();
}

export async function getPeers(): Promise<PeersResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/peers`);
  return res.json();
}

export async function getSessions(params?: ListSessionsParams): Promise<Session[]> {
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
  const base = address.includes('://') ? address : `http://${address}`;
  const res = await authFetch(`${base}/api/v1/sessions`);
  return res.json();
}

export async function getSession(id: string): Promise<Session> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}`);
  return res.json();
}

export async function createSession(data: CreateSessionRequest): Promise<CreateSessionResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to create session');
  }
  return res.json();
}

export async function createRemoteSession(
  address: string,
  data: CreateSessionRequest,
): Promise<CreateSessionResponse> {
  const base = address.includes('://') ? address : `http://${address}`;
  const res = await authFetch(`${base}/api/v1/sessions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to create session');
  }
  return res.json();
}

export async function killSession(id: string): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}/kill`, { method: 'POST' });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to kill session');
  }
}

export async function deleteSession(id: string): Promise<void> {
  await authFetch(`${resolveBaseUrl()}/sessions/${id}`, { method: 'DELETE' });
}

export async function getSessionOutput(
  id: string,
  lines: number = 100,
): Promise<{ output: string }> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}/output?lines=${lines}`);
  return res.json();
}

export async function sendInput(id: string, text: string): Promise<void> {
  await authFetch(`${resolveBaseUrl()}/sessions/${id}/input`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text }),
  });
}

export async function resumeSession(id: string): Promise<{ id: string; status: string }> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${id}/resume`, { method: 'POST' });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to resume session');
  }
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

export async function getConfig(): Promise<ConfigResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/config`);
  return res.json();
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

export async function updateRemoteConfig(
  address: string,
  data: UpdateConfigRequest,
): Promise<UpdateConfigResponse> {
  const base = address.includes('://') ? address : `http://${address}`;
  const res = await authFetch(`${base}/api/v1/config`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to update remote config');
  }
  return res.json();
}

export async function listKnowledge(params?: {
  session_id?: string;
  kind?: string;
  repo?: string;
  ink?: string;
  limit?: number;
}): Promise<KnowledgeResponse> {
  const query = new URLSearchParams();
  if (params?.session_id) query.set('session_id', params.session_id);
  if (params?.kind) query.set('kind', params.kind);
  if (params?.repo) query.set('repo', params.repo);
  if (params?.ink) query.set('ink', params.ink);
  if (params?.limit) query.set('limit', String(params.limit));
  const qs = query.toString();
  const res = await authFetch(`${resolveBaseUrl()}/knowledge${qs ? `?${qs}` : ''}`);
  if (!res.ok) throw new Error('Failed to fetch knowledge');
  return res.json();
}

export async function getKnowledgeContext(params?: {
  workdir?: string;
  ink?: string;
  limit?: number;
}): Promise<KnowledgeResponse> {
  const query = new URLSearchParams();
  if (params?.workdir) query.set('workdir', params.workdir);
  if (params?.ink) query.set('ink', params.ink);
  if (params?.limit) query.set('limit', String(params.limit));
  const qs = query.toString();
  const res = await authFetch(`${resolveBaseUrl()}/knowledge/context${qs ? `?${qs}` : ''}`);
  if (!res.ok) throw new Error('Failed to fetch knowledge context');
  return res.json();
}

export async function getKnowledgeItem(id: string): Promise<KnowledgeItemResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/knowledge/${id}`);
  if (!res.ok) throw new Error('Failed to fetch knowledge item');
  return res.json();
}

export async function updateKnowledge(
  id: string,
  data: UpdateKnowledgeRequest,
): Promise<KnowledgeItemResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/knowledge/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  if (!res.ok) throw new Error('Failed to update knowledge item');
  return res.json();
}

export async function deleteKnowledge(id: string): Promise<KnowledgeDeleteResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/knowledge/${id}`, { method: 'DELETE' });
  if (!res.ok) throw new Error('Failed to delete knowledge item');
  return res.json();
}

export async function pushKnowledge(): Promise<KnowledgePushResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/knowledge/push`, { method: 'POST' });
  if (!res.ok) throw new Error('Failed to push knowledge');
  return res.json();
}
