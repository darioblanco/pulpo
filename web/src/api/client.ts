import type {
  Session,
  UsageSessionsResponse,
  NodeInfo,
  ListSessionsParams,
  InterventionEvent,
  ConfigResponse,
  CreateSessionRequest,
  CreateSessionResponse,
  CleanupSessionsResponse,
  ScheduleInfo,
  CreateScheduleRequest,
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

/** Extract a meaningful error message from a failed API response. */
async function apiError(res: Response, fallback: string): Promise<Error> {
  try {
    const body = await res.text();
    try {
      const json = JSON.parse(body);
      const msg = json.error || json.message;
      if (msg) return new Error(msg);
    } catch {
      if (body) return new Error(body);
    }
  } catch {
    // body read failed
  }
  return new Error(fallback);
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

export async function getNode(): Promise<NodeInfo> {
  const res = await authFetch(`${resolveBaseUrl()}/node`);
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

export async function getUsageSessions(): Promise<UsageSessionsResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/usage/sessions`);
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
  if (!res.ok) throw await apiError(res, 'Failed to create session');
  return res.json();
}

export async function stopSession(id: string, purge?: boolean): Promise<void> {
  const url = `${resolveBaseUrl()}/sessions/${id}/stop${purge ? '?purge=true' : ''}`;
  const res = await authFetch(url, { method: 'POST' });
  if (!res.ok) throw await apiError(res, 'Failed to stop session');
}

/**
 * Remove a single session outright (`DELETE /api/v1/sessions/{id}`). Only sessions
 * not currently active/idle may be removed — the daemon returns 409 otherwise
 * (stop it first). Purges the session row, its intervention events, exit markers,
 * session log, and harness dir.
 */
export async function removeSession(id: string): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!res.ok) throw await apiError(res, 'Failed to remove session');
}

export async function cleanupSessions(): Promise<CleanupSessionsResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/sessions/cleanup`, { method: 'POST' });
  if (!res.ok) throw await apiError(res, 'Failed to cleanup sessions');
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
  if (!res.ok) throw await apiError(res, 'Failed to resume session');
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

/** Read-only effective config — the web UI never writes it back. Edit
 * `~/.pulpo/config.toml` and restart pulpod to change anything. */
export async function getConfig(): Promise<ConfigResponse> {
  const res = await authFetch(`${resolveBaseUrl()}/config`);
  return res.json();
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
  if (!res.ok) throw await apiError(res, 'Failed to create schedule');
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
  if (!res.ok) throw await apiError(res, 'Failed to update schedule');
  return res.json();
}

export async function getScheduleRuns(id: string): Promise<Session[]> {
  const res = await authFetch(`${resolveBaseUrl()}/schedules/${encodeURIComponent(id)}/runs`);
  if (!res.ok) throw await apiError(res, 'Failed to fetch schedule runs');
  return res.json();
}

export async function deleteSchedule(id: string): Promise<void> {
  const res = await authFetch(`${resolveBaseUrl()}/schedules/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!res.ok) throw await apiError(res, 'Failed to delete schedule');
}
