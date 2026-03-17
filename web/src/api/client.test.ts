import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  setApiConfig,
  getNode,
  getPeers,
  getSessions,
  getRemoteSessions,
  getSession,
  createSession,
  createRemoteSession,
  killSession,
  deleteSession,
  getSessionOutput,
  downloadSessionOutput,
  sendInput,
  resumeSession,
  getInterventionEvents,
  getConfig,
  updateConfig,
  updateRemoteConfig,
  addPeer,
  removePeer,
  getPairingUrl,
  getInks,
  resolveWsUrl,
  resolveBaseUrl,
  authHeaders,
} from './client';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

let testUrl = '';
let testToken = '';

beforeEach(() => {
  mockFetch.mockReset();
  testUrl = '';
  testToken = '';
  setApiConfig({
    getBaseUrl: () => testUrl,
    getAuthToken: () => testToken,
  });
});

function jsonResponse(data: unknown) {
  return { json: () => Promise.resolve(data) };
}

describe('getNode', () => {
  it('fetches /api/v1/node with relative URL', async () => {
    const node = { name: 'mac-mini', hostname: 'mac-mini.local' };
    mockFetch.mockResolvedValue(jsonResponse(node));

    const result = await getNode();

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/node', { headers: {} });
    expect(result).toEqual(node);
  });

  it('fetches from absolute URL when base is set', async () => {
    testUrl = 'http://mac-mini:7433';
    const node = { name: 'mac-mini', hostname: 'mac-mini.local' };
    mockFetch.mockResolvedValue(jsonResponse(node));

    const result = await getNode();

    expect(mockFetch).toHaveBeenCalledWith('http://mac-mini:7433/api/v1/node', { headers: {} });
    expect(result).toEqual(node);
  });

  it('includes auth header when token is set', async () => {
    testToken = 'my-secret';
    const node = { name: 'mac-mini' };
    mockFetch.mockResolvedValue(jsonResponse(node));

    await getNode();

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/node', {
      headers: { Authorization: 'Bearer my-secret' },
    });
  });
});

describe('getPeers', () => {
  it('fetches /api/v1/peers', async () => {
    const peers = { local: { name: 'mac-mini' }, peers: [] };
    mockFetch.mockResolvedValue(jsonResponse(peers));

    const result = await getPeers();

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/peers', { headers: {} });
    expect(result).toEqual(peers);
  });
});

describe('getSessions', () => {
  it('fetches /api/v1/sessions without params', async () => {
    const sessions = [{ id: '1', name: 'test' }];
    mockFetch.mockResolvedValue(jsonResponse(sessions));

    const result = await getSessions();

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions', { headers: {} });
    expect(result).toEqual(sessions);
  });

  it('fetches with filter params', async () => {
    const sessions = [{ id: '1', name: 'test' }];
    mockFetch.mockResolvedValue(jsonResponse(sessions));

    const result = await getSessions({ status: 'active', search: 'claude' });

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions?status=active&search=claude', {
      headers: {},
    });
    expect(result).toEqual(sessions);
  });

  it('ignores undefined params', async () => {
    const sessions: unknown[] = [];
    mockFetch.mockResolvedValue(jsonResponse(sessions));

    await getSessions({ status: 'ready', search: undefined });

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions?status=ready', { headers: {} });
  });

  it('uses absolute URL when base is set', async () => {
    testUrl = 'http://mac-mini:7433';
    const sessions = [{ id: '1', name: 'test' }];
    mockFetch.mockResolvedValue(jsonResponse(sessions));

    const result = await getSessions({ status: 'active' });

    expect(mockFetch).toHaveBeenCalledWith('http://mac-mini:7433/api/v1/sessions?status=active', {
      headers: {},
    });
    expect(result).toEqual(sessions);
  });

  it('uses absolute URL without params when base is set', async () => {
    testUrl = 'http://mac-mini:7433';
    const sessions: unknown[] = [];
    mockFetch.mockResolvedValue(jsonResponse(sessions));

    await getSessions();

    expect(mockFetch).toHaveBeenCalledWith('http://mac-mini:7433/api/v1/sessions', {
      headers: {},
    });
  });
});

describe('getRemoteSessions', () => {
  it('fetches sessions from remote address', async () => {
    const sessions = [{ id: '2', name: 'remote-test' }];
    mockFetch.mockResolvedValue(jsonResponse(sessions));

    const result = await getRemoteSessions('win-pc:7433');

    expect(mockFetch).toHaveBeenCalledWith('http://win-pc:7433/api/v1/sessions', { headers: {} });
    expect(result).toEqual(sessions);
  });
});

describe('getSession', () => {
  it('fetches a single session by id', async () => {
    const session = { id: 'abc', name: 'my-session' };
    mockFetch.mockResolvedValue(jsonResponse(session));

    const result = await getSession('abc');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc', { headers: {} });
    expect(result).toEqual(session);
  });
});

describe('createSession', () => {
  it('posts to /api/v1/sessions with JSON body', async () => {
    const created = { id: 'new-1', name: 'my-api' };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(created) });

    const data = {
      name: 'my-session',
      workdir: '/home/user/repo',
      command: 'claude code',
      description: 'Fix the bug',
    };
    const result = await createSession(data);

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(data),
    });
    expect(result).toEqual(created);
  });

  it('includes auth header with Content-Type when token is set', async () => {
    testToken = 'post-token';
    const created = { id: 'new-1', name: 'my-api' };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(created) });

    const data = { name: 'auth-test', workdir: '/repo', command: 'claude code' };
    await createSession(data);

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: 'Bearer post-token' },
      body: JSON.stringify(data),
    });
  });

  it('throws on error response', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ error: 'working directory does not exist: /bad/path' }),
    });

    await expect(
      createSession({ name: 'err-test', workdir: '/bad/path', command: 'test' }),
    ).rejects.toThrow('working directory does not exist');
  });

  it('throws generic message when no error field', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({}),
    });

    await expect(
      createSession({ name: 'gen-err', workdir: '/repo', command: 'test' }),
    ).rejects.toThrow('Failed to create session');
  });
});

describe('createRemoteSession', () => {
  it('posts to remote address with JSON body', async () => {
    const created = { id: 'remote-1', name: 'remote-api' };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(created) });

    const data = { name: 'remote-test', workdir: '/repo', command: 'claude code' };
    const result = await createRemoteSession('macbook:7433', data);

    expect(mockFetch).toHaveBeenCalledWith('http://macbook:7433/api/v1/sessions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(data),
    });
    expect(result).toEqual(created);
  });

  it('throws on error response', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ error: 'provider not installed' }),
    });

    await expect(
      createRemoteSession('macbook:7433', {
        name: 'remote-err',
        workdir: '/repo',
        command: 'test',
      }),
    ).rejects.toThrow('provider not installed');
  });
});

describe('killSession', () => {
  it('sends POST to /api/v1/sessions/:id/kill', async () => {
    mockFetch.mockResolvedValue({ ok: true });

    await killSession('abc');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc/kill', {
      method: 'POST',
      headers: {},
    });
  });

  it('throws on error response', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ error: 'session not found' }),
    });

    await expect(killSession('abc')).rejects.toThrow('session not found');
  });

  it('throws generic message when no error field', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({}),
    });

    await expect(killSession('abc')).rejects.toThrow('Failed to kill session');
  });
});

describe('deleteSession', () => {
  it('sends DELETE to /api/v1/sessions/:id', async () => {
    mockFetch.mockResolvedValue({});

    await deleteSession('abc');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc', {
      method: 'DELETE',
      headers: {},
    });
  });
});

describe('getSessionOutput', () => {
  it('fetches output with default lines=100', async () => {
    const output = { output: 'some terminal output' };
    mockFetch.mockResolvedValue(jsonResponse(output));

    const result = await getSessionOutput('abc');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc/output?lines=100', {
      headers: {},
    });
    expect(result).toEqual(output);
  });

  it('fetches output with custom lines parameter', async () => {
    const output = { output: 'more output' };
    mockFetch.mockResolvedValue(jsonResponse(output));

    const result = await getSessionOutput('abc', 50);

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc/output?lines=50', {
      headers: {},
    });
    expect(result).toEqual(output);
  });
});

describe('downloadSessionOutput', () => {
  it('fetches blob from /api/v1/sessions/:id/output/download', async () => {
    const blob = new Blob(['log output'], { type: 'text/plain' });
    mockFetch.mockResolvedValue({ blob: () => Promise.resolve(blob) });

    const result = await downloadSessionOutput('abc');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc/output/download', {
      headers: {},
    });
    expect(result).toBe(blob);
  });
});

describe('sendInput', () => {
  it('posts input text to /api/v1/sessions/:id/input', async () => {
    mockFetch.mockResolvedValue({});

    await sendInput('abc', 'hello\n');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc/input', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: 'hello\n' }),
    });
  });
});

describe('resumeSession', () => {
  it('posts to /api/v1/sessions/:id/resume', async () => {
    const resumed = { id: 'abc', status: 'active' };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(resumed) });

    const result = await resumeSession('abc');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc/resume', {
      method: 'POST',
      headers: {},
    });
    expect(result).toEqual(resumed);
  });

  it('throws on error response', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ error: 'session is not lost' }),
    });

    await expect(resumeSession('abc')).rejects.toThrow('session is not lost');
  });

  it('throws generic message when no error field', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({}),
    });

    await expect(resumeSession('abc')).rejects.toThrow('Failed to resume session');
  });
});

describe('getInterventionEvents', () => {
  it('fetches /api/v1/sessions/:id/interventions', async () => {
    const events = [
      { id: 1, session_id: 'abc', reason: 'OOM', created_at: '2026-01-01T00:00:00Z' },
    ];
    mockFetch.mockResolvedValue(jsonResponse(events));

    const result = await getInterventionEvents('abc');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/sessions/abc/interventions', { headers: {} });
    expect(result).toEqual(events);
  });

  it('returns empty array when no events', async () => {
    mockFetch.mockResolvedValue(jsonResponse([]));

    const result = await getInterventionEvents('abc');

    expect(result).toEqual([]);
  });
});

describe('getConfig', () => {
  it('fetches /api/v1/config', async () => {
    const config = {
      node: { name: 'mac-mini', port: 7433, data_dir: '~/.pulpo' },
      peers: {},
      guards: { preset: 'standard' },
    };
    mockFetch.mockResolvedValue(jsonResponse(config));

    const result = await getConfig();

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/config', { headers: {} });
    expect(result).toEqual(config);
  });
});

describe('updateConfig', () => {
  it('sends PUT to /api/v1/config with partial update', async () => {
    const response = {
      config: {
        node: { name: 'new-name', port: 7433, data_dir: '~/.pulpo' },
        peers: {},
        guards: { preset: 'standard' },
      },
      restart_required: false,
    };
    mockFetch.mockResolvedValue(jsonResponse(response));

    const result = await updateConfig({ node_name: 'new-name' });

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/config', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ node_name: 'new-name' }),
    });
    expect(result).toEqual(response);
    expect(result.restart_required).toBe(false);
  });

  it('reports restart_required when port changes', async () => {
    const response = {
      config: {
        node: { name: 'mac-mini', port: 9000, data_dir: '~/.pulpo' },
        peers: {},
        guards: { preset: 'standard' },
      },
      restart_required: true,
    };
    mockFetch.mockResolvedValue(jsonResponse(response));

    const result = await updateConfig({ port: 9000 });

    expect(result.restart_required).toBe(true);
  });
});

describe('updateRemoteConfig', () => {
  it('sends PUT to remote address /api/v1/config', async () => {
    const response = {
      config: { node: { name: 'remote' }, peers: {}, guards: { preset: 'standard' } },
      restart_required: false,
    };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(response) });

    const data = {
      inks: {
        reviewer: {
          description: 'Test',
          command: 'claude code',
        },
      },
    };
    const result = await updateRemoteConfig('macbook:7433', data);

    expect(mockFetch).toHaveBeenCalledWith('http://macbook:7433/api/v1/config', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(data),
    });
    expect(result).toEqual(response);
  });

  it('uses scheme from address when present', async () => {
    const response = { config: {}, restart_required: false };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(response) });

    await updateRemoteConfig('https://remote:7433', { inks: {} });

    expect(mockFetch).toHaveBeenCalledWith('https://remote:7433/api/v1/config', expect.anything());
  });

  it('throws on error response', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ error: 'unauthorized' }),
    });

    await expect(updateRemoteConfig('macbook:7433', {})).rejects.toThrow('unauthorized');
  });

  it('throws generic message when no error field', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({}),
    });

    await expect(updateRemoteConfig('macbook:7433', {})).rejects.toThrow(
      'Failed to update remote config',
    );
  });
});

describe('addPeer', () => {
  it('posts to /api/v1/peers with name and address', async () => {
    const resp = { local: {}, peers: [{ name: 'new', address: '10.0.0.1:7433' }] };
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve(resp) });

    const result = await addPeer('new', '10.0.0.1:7433');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/peers', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'new', address: '10.0.0.1:7433' }),
    });
    expect(result).toEqual(resp);
  });

  it('throws on conflict', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ error: 'already exists' }),
    });

    await expect(addPeer('dup', 'x:7433')).rejects.toThrow('already exists');
  });
});

describe('removePeer', () => {
  it('sends DELETE to /api/v1/peers/:name', async () => {
    mockFetch.mockResolvedValue({ ok: true });

    await removePeer('old-node');

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/peers/old-node', {
      method: 'DELETE',
      headers: {},
    });
  });

  it('throws on not found', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      json: () => Promise.resolve({ error: 'not found' }),
    });

    await expect(removePeer('missing')).rejects.toThrow('not found');
  });
});

describe('getPairingUrl', () => {
  it('fetches /api/v1/auth/pairing-url', async () => {
    const resp = { url: 'http://mac-mini:7433/?token=abc123' };
    mockFetch.mockResolvedValue(jsonResponse(resp));

    const result = await getPairingUrl();

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/auth/pairing-url', { headers: {} });
    expect(result).toEqual(resp);
  });
});

describe('getInks', () => {
  it('fetches /api/v1/inks', async () => {
    const resp = { inks: { coder: { description: null, command: 'claude code' } } };
    mockFetch.mockResolvedValue(jsonResponse(resp));

    const result = await getInks();

    expect(mockFetch).toHaveBeenCalledWith('/api/v1/inks', { headers: {} });
    expect(result).toEqual(resp);
  });
});

describe('resolveBaseUrl', () => {
  it('returns relative path when no base URL set', () => {
    expect(resolveBaseUrl()).toBe('/api/v1');
  });

  it('returns absolute URL when base is set', () => {
    testUrl = 'http://mac-mini:7433';
    expect(resolveBaseUrl()).toBe('http://mac-mini:7433/api/v1');
  });
});

describe('authHeaders', () => {
  it('returns empty object when no token set', () => {
    expect(authHeaders()).toEqual({});
  });

  it('includes Authorization header when token is set', () => {
    testToken = 'secret';
    expect(authHeaders()).toEqual({ Authorization: 'Bearer secret' });
  });

  it('merges extra headers with auth header', () => {
    testToken = 'secret';
    const result = authHeaders({ 'Content-Type': 'application/json' });
    expect(result).toEqual({
      'Content-Type': 'application/json',
      Authorization: 'Bearer secret',
    });
  });
});

describe('resolveWsUrl', () => {
  it('uses ws: and location.host when no base URL', () => {
    vi.stubGlobal('location', { protocol: 'http:', host: 'localhost:7433' });
    const url = resolveWsUrl('/api/v1/sessions/s1/stream');
    expect(url).toBe('ws://localhost:7433/api/v1/sessions/s1/stream');
  });

  it('uses wss: for https protocol', () => {
    vi.stubGlobal('location', { protocol: 'https:', host: 'example.com' });
    const url = resolveWsUrl('/api/v1/sessions/s1/stream');
    expect(url).toBe('wss://example.com/api/v1/sessions/s1/stream');
    vi.stubGlobal('location', { protocol: 'http:', host: 'localhost:7433' });
  });

  it('uses base URL host when set (http → ws)', () => {
    testUrl = 'http://mac-mini:7433';
    const url = resolveWsUrl('/api/v1/sessions/s1/stream');
    expect(url).toBe('ws://mac-mini:7433/api/v1/sessions/s1/stream');
  });

  it('uses base URL host when set (https → wss)', () => {
    testUrl = 'https://remote:7433';
    const url = resolveWsUrl('/api/v1/sessions/s1/stream');
    expect(url).toBe('wss://remote:7433/api/v1/sessions/s1/stream');
  });

  it('appends token as query param when set', () => {
    testToken = 'ws-token';
    vi.stubGlobal('location', { protocol: 'http:', host: 'localhost:7433' });
    const url = resolveWsUrl('/api/v1/sessions/s1/stream');
    expect(url).toBe('ws://localhost:7433/api/v1/sessions/s1/stream?token=ws-token');
  });

  it('appends token with base URL', () => {
    testUrl = 'http://mac-mini:7433';
    testToken = 'remote-token';
    const url = resolveWsUrl('/api/v1/sessions/s1/stream');
    expect(url).toBe('ws://mac-mini:7433/api/v1/sessions/s1/stream?token=remote-token');
  });
});
