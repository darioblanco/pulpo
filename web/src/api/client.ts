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
  CleanupSessionsResponse,
  FleetSessionsResponse,
  VapidPublicKeyResponse,
  ScheduleInfo,
  CreateScheduleRequest,
  SecretEntry,
  SecretListResponse,
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

export async function getFleetSessions(): Promise<FleetSessionsResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/fleet/sessions`);
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

export async function stopSession(id: string, purge?: boolean): Promise<void> {
  const url = `${resolveBaseUrl()}/sessions/${id}/stop${purge ? '?purge=true' : ''}`;
  const res = await authFetch(url, { method: 'POST' });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to stop session');
  }
}

export async function cleanupSessions(): Promise<CleanupSessionsResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/cleanup`, { method: 'POST' });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to cleanup sessions');
  }
  return res.json();
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

export async function getVapidKey(): Promise<VapidPublicKeyResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/push/vapid-key`);
  if (!res.ok) throw new Error('Failed to get VAPID key');
  return res.json();
}

export async function subscribePush(subscription: PushSubscriptionJSON): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/push/subscribe`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      endpoint: subscription.endpoint,
      keys: subscription.keys,
    }),
  });
  if (!res.ok) throw new Error('Failed to subscribe');
}

export async function unsubscribePush(endpoint: string): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/push/unsubscribe`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ endpoint }),
  });
  if (!res.ok) throw new Error('Failed to unsubscribe');
}

export async function getSchedules(): Promise<ScheduleInfo[]> {
  const res = await authFetch(`${resolveBaseUrl()}/schedules`);
  return res.json();
}

export async function createSchedule(data: CreateScheduleRequest): Promise<ScheduleInfo> {
  const res = await authFetch(`${resolveBaseUrl()}/schedules`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to create schedule');
  }
  return res.json();
}

export async function updateSchedule(
  id: string,
  data: Record<string, unknown>,
): Promise<ScheduleInfo> {
  const res = await authFetch(`${resolveBaseUrl()}/schedules/${encodeURIComponent(id)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to update schedule');
  }
  return res.json();
}

export async function getScheduleRuns(id: string): Promise<Session[]> {
  const res = await authFetch(`${resolveBaseUrl()}/schedules/${encodeURIComponent(id)}/runs`);
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to fetch schedule runs');
  }
  return res.json();
}

export async function deleteSchedule(id: string): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/schedules/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to delete schedule');
  }
}

// -- Secrets API --

export async function getSecrets(): Promise<SecretEntry[]> {
  const res = await authFetch(`${resolveBaseUrl()}/secrets`);
  const data: SecretListResponse = await res.json();
  return data.secrets;
}

export async function setSecret(name: string, value: string, env?: string): Promise<void> {
  const body: Record<string, string> = { value };
  if (env) body.env = env;
  const res = await authFetch(`${resolveBaseUrl()}/secrets/${encodeURIComponent(name)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to set secret');
  }
}

export async function deleteSecret(name: string): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/secrets/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || 'Failed to delete secret');
  }
}
