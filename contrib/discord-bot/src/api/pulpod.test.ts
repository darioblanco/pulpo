import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { PulpodClient } from './pulpod.js';

describe('PulpodClient', () => {
  const config = {
    discordToken: 'discord-token',
    pulpodUrl: 'http://localhost:7433',
    pulpodToken: 'test-api-token',
  };

  let mockFetch: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockFetch = vi.fn();
    vi.stubGlobal('fetch', mockFetch);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('constructs correct base URL stripping trailing slashes', () => {
    const client = new PulpodClient({ ...config, pulpodUrl: 'http://host:8080///' });
    expect(client.sseUrl()).toContain('http://host:8080/api/v1/events');
  });

  it('sseUrl includes token parameter', () => {
    const client = new PulpodClient(config);
    expect(client.sseUrl()).toBe('http://localhost:7433/api/v1/events?token=test-api-token');
  });

  it('sseUrl omits token if empty', () => {
    const client = new PulpodClient({ ...config, pulpodToken: '' });
    expect(client.sseUrl()).toBe('http://localhost:7433/api/v1/events');
  });

  it('listSessions calls correct endpoint', async () => {
    mockFetch.mockResolvedValue({
      ok: true,
      json: () => Promise.resolve([]),
    });

    const client = new PulpodClient(config);
    const sessions = await client.listSessions();

    expect(sessions).toEqual([]);
    expect(mockFetch).toHaveBeenCalledWith('http://localhost:7433/api/v1/sessions', {
      headers: {
        'Content-Type': 'application/json',
        Authorization: 'Bearer test-api-token',
      },
    });
  });

  it('listSessions throws on non-ok response', async () => {
    mockFetch.mockResolvedValue({ ok: false, status: 500 });

    const client = new PulpodClient(config);
    await expect(client.listSessions()).rejects.toThrow('Failed to list sessions: 500');
  });

  it('getSession calls correct endpoint', async () => {
    const session = { id: 'abc', name: 'test', status: 'running' };
    mockFetch.mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(session),
    });

    const client = new PulpodClient(config);
    const result = await client.getSession('abc');
    expect(result).toEqual(session);
    expect(mockFetch).toHaveBeenCalledWith(
      'http://localhost:7433/api/v1/sessions/abc',
      expect.any(Object),
    );
  });

  it('getSession throws on non-ok response', async () => {
    mockFetch.mockResolvedValue({ ok: false, status: 404 });

    const client = new PulpodClient(config);
    await expect(client.getSession('missing')).rejects.toThrow('Failed to get session: 404');
  });

  it('createSession sends POST with body', async () => {
    const session = { id: 'new', name: 'test', status: 'creating' };
    mockFetch.mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(session),
    });

    const client = new PulpodClient(config);
    const result = await client.createSession({
      name: 'test-session',
      workdir: '/code/repo',
      command: 'claude "fix the bug"',
      ink: 'coder',
    });

    expect(result).toEqual(session);
    expect(mockFetch).toHaveBeenCalledWith('http://localhost:7433/api/v1/sessions', {
      method: 'POST',
      headers: expect.objectContaining({ Authorization: 'Bearer test-api-token' }),
      body: JSON.stringify({
        name: 'test-session',
        workdir: '/code/repo',
        command: 'claude "fix the bug"',
        ink: 'coder',
      }),
    });
  });

  it('createSession throws with body on non-ok response', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      status: 400,
      text: () => Promise.resolve('{"error":"bad request"}'),
    });

    const client = new PulpodClient(config);
    await expect(client.createSession({ name: 'test' })).rejects.toThrow(
      'Failed to create session (400)',
    );
  });

  it('stopSession sends POST to /stop', async () => {
    mockFetch.mockResolvedValue({ ok: true });

    const client = new PulpodClient(config);
    await client.stopSession('abc');

    expect(mockFetch).toHaveBeenCalledWith('http://localhost:7433/api/v1/sessions/abc/stop', {
      method: 'POST',
      headers: expect.any(Object),
    });
  });

  it('stopSession throws on non-ok response', async () => {
    mockFetch.mockResolvedValue({ ok: false, status: 404 });

    const client = new PulpodClient(config);
    await expect(client.stopSession('missing')).rejects.toThrow('Failed to stop session: 404');
  });

  it('getOutput returns text', async () => {
    mockFetch.mockResolvedValue({
      ok: true,
      text: () => Promise.resolve('hello world'),
    });

    const client = new PulpodClient(config);
    const output = await client.getOutput('abc', 100);

    expect(output).toBe('hello world');
    expect(mockFetch).toHaveBeenCalledWith(
      'http://localhost:7433/api/v1/sessions/abc/output?lines=100',
      expect.any(Object),
    );
  });

  it('getOutput without lines param omits query string', async () => {
    mockFetch.mockResolvedValue({
      ok: true,
      text: () => Promise.resolve('output'),
    });

    const client = new PulpodClient(config);
    await client.getOutput('abc');

    expect(mockFetch).toHaveBeenCalledWith(
      'http://localhost:7433/api/v1/sessions/abc/output',
      expect.any(Object),
    );
  });

  it('getOutput throws on non-ok response', async () => {
    mockFetch.mockResolvedValue({ ok: false, status: 500 });

    const client = new PulpodClient(config);
    await expect(client.getOutput('abc')).rejects.toThrow('Failed to get output: 500');
  });

  it('listInks calls correct endpoint', async () => {
    const data = { inks: { coder: { description: 'Autonomous coder', command: 'claude' } } };
    mockFetch.mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(data),
    });

    const client = new PulpodClient(config);
    const result = await client.listInks();
    expect(result.inks.coder.command).toBe('claude');
  });

  it('listInks throws on non-ok response', async () => {
    mockFetch.mockResolvedValue({ ok: false, status: 403 });

    const client = new PulpodClient(config);
    await expect(client.listInks()).rejects.toThrow('Failed to list inks: 403');
  });

  it('resumeSession sends POST to /resume', async () => {
    const session = { id: 'abc', name: 'test', status: 'running' };
    mockFetch.mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(session),
    });

    const client = new PulpodClient(config);
    const result = await client.resumeSession('abc');

    expect(result).toEqual(session);
    expect(mockFetch).toHaveBeenCalledWith('http://localhost:7433/api/v1/sessions/abc/resume', {
      method: 'POST',
      headers: expect.objectContaining({ Authorization: 'Bearer test-api-token' }),
    });
  });

  it('resumeSession throws with body on non-ok response', async () => {
    mockFetch.mockResolvedValue({
      ok: false,
      status: 409,
      text: () => Promise.resolve('{"error":"session is running"}'),
    });

    const client = new PulpodClient(config);
    await expect(client.resumeSession('abc')).rejects.toThrow('Failed to resume session (409)');
  });

  it('sendInput sends POST with text body', async () => {
    mockFetch.mockResolvedValue({ ok: true });

    const client = new PulpodClient(config);
    await client.sendInput('abc', 'yes');

    expect(mockFetch).toHaveBeenCalledWith('http://localhost:7433/api/v1/sessions/abc/input', {
      method: 'POST',
      headers: expect.objectContaining({ Authorization: 'Bearer test-api-token' }),
      body: JSON.stringify({ text: 'yes' }),
    });
  });

  it('sendInput throws on non-ok response', async () => {
    mockFetch.mockResolvedValue({ ok: false, status: 404 });

    const client = new PulpodClient(config);
    await expect(client.sendInput('missing', 'text')).rejects.toThrow('Failed to send input: 404');
  });

  it('headers omit Authorization when token is empty', async () => {
    mockFetch.mockResolvedValue({ ok: true, json: () => Promise.resolve([]) });

    const client = new PulpodClient({ ...config, pulpodToken: '' });
    await client.listSessions();

    const headers = mockFetch.mock.calls[0][1].headers;
    expect(headers).not.toHaveProperty('Authorization');
    expect(headers['Content-Type']).toBe('application/json');
  });
});
