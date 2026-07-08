import { describe, it, expect, vi } from 'vitest';
import {
  STOP_ACTION,
  buildNotificationOptions,
  buildStopResultNotification,
  postStopAction,
  type PushPayload,
} from './push-sw';

describe('buildNotificationOptions', () => {
  it('builds title/body/icon/badge from the payload', () => {
    const payload: PushPayload = {
      title: 'Session: fix-auth',
      body: 'Session `fix-auth` is now ready',
      url: '/sessions/abc-123',
      session_id: 'abc-123',
    };
    const { title, options } = buildNotificationOptions(payload);
    expect(title).toBe('Session: fix-auth');
    expect(options.body).toBe('Session `fix-auth` is now ready');
    expect(options.icon).toBe('/icons/icon-192x192.png');
    expect(options.badge).toBe('/icons/icon-192x192.png');
    expect(options.data).toEqual({ url: '/sessions/abc-123', actionToken: undefined });
    expect(options.tag).toBe('pulpo-session-abc-123');
    expect(options.actions).toBeUndefined();
  });

  it('falls back to defaults when fields are missing', () => {
    const { title, options } = buildNotificationOptions({});
    expect(title).toBe('Pulpo');
    expect(options.body).toBe('');
    expect(options.data).toEqual({ url: '/', actionToken: undefined });
    expect(options.tag).toBe('pulpo-notification');
  });

  it('attaches a "Stop session" action when the payload carries a token', () => {
    const payload: PushPayload = {
      title: 'Budget alert: fix-auth',
      body: 'fix-auth at 82% ($8.20/$10.00)',
      session_id: 'sess-42',
      action: { token: 'signed-token', label: 'Stop session' },
    };
    const { options } = buildNotificationOptions(payload);
    expect(options.actions).toEqual([{ action: STOP_ACTION, title: 'Stop session' }]);
    expect(options.data).toEqual({
      url: '/',
      actionToken: 'signed-token',
    });
  });

  it('falls back to a default action label when the payload label is empty', () => {
    const payload: PushPayload = {
      session_id: 'sess-1',
      action: { token: 't', label: '' },
    };
    const { options } = buildNotificationOptions(payload);
    expect(options.actions).toEqual([{ action: STOP_ACTION, title: 'Stop session' }]);
  });

  it('omits actions entirely for lifecycle/intervention payloads (no action field)', () => {
    const { options } = buildNotificationOptions({ title: 'Intervention: fix-auth' });
    expect(options.actions).toBeUndefined();
  });
});

describe('postStopAction', () => {
  it('POSTs the token to /api/v1/push/action resolved against the scope', async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({ session_id: 'sess-1', session_name: 'fix-auth' }),
    });

    const result = await postStopAction('https://pulpo.example.com/', 'signed-token', fetchImpl);

    expect(fetchImpl).toHaveBeenCalledWith(
      'https://pulpo.example.com/api/v1/push/action',
      expect.objectContaining({
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ token: 'signed-token' }),
      }),
    );
    expect(result).toEqual({ success: true, sessionName: 'fix-auth' });
  });

  it('returns success without a session name when the body has none', async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({}),
    });

    const result = await postStopAction('https://pulpo.example.com/', 't', fetchImpl);
    expect(result).toEqual({ success: true, sessionName: undefined });
  });

  it('returns success:false on a non-2xx response (e.g. 401/410)', async () => {
    const fetchImpl = vi.fn().mockResolvedValue({ ok: false });
    const result = await postStopAction('https://pulpo.example.com/', 'bad-token', fetchImpl);
    expect(result).toEqual({ success: false });
  });

  it('returns success:false when the response body is not valid JSON', async () => {
    const fetchImpl = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.reject(new Error('not json')),
    });
    const result = await postStopAction('https://pulpo.example.com/', 't', fetchImpl);
    expect(result).toEqual({ success: true, sessionName: undefined });
  });

  it('returns success:false when fetch throws (offline, network error)', async () => {
    const fetchImpl = vi.fn().mockRejectedValue(new Error('network down'));
    const result = await postStopAction('https://pulpo.example.com/', 't', fetchImpl);
    expect(result).toEqual({ success: false });
  });
});

describe('buildStopResultNotification', () => {
  it('builds a success confirmation with the session name', () => {
    const { title, options } = buildStopResultNotification({
      success: true,
      sessionName: 'fix-auth',
    });
    expect(title).toBe('Session stopped');
    expect(options.body).toBe('fix-auth was stopped.');
  });

  it('builds a generic success confirmation without a session name', () => {
    const { title, options } = buildStopResultNotification({ success: true });
    expect(title).toBe('Session stopped');
    expect(options.body).toBe('Session was stopped.');
  });

  it('builds a failure confirmation', () => {
    const { title, options } = buildStopResultNotification({ success: false });
    expect(title).toBe('Stop failed');
    expect(options.body).toMatch(/could not stop/i);
  });
});
