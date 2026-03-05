import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  detectStatusChanges,
  formatStatusLabel,
  processSessionChanges,
  requestNotificationPermission,
  showDesktopNotification,
  type StatusChange,
} from './notifications';
import type { Session } from '@/api/types';

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix the bug',
    mode: 'interactive',
    workdir: '/home/user/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    persona: null,
    intervention_reason: null,
    intervention_at: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    last_output_at: null,
    waiting_for_input: false,
    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('detectStatusChanges', () => {
  it('detects running → completed transition', () => {
    const prev = [makeSession({ id: '1', status: 'running' })];
    const curr = [makeSession({ id: '1', status: 'completed' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'running',
      to: 'completed',
    });
  });

  it('detects running → dead transition', () => {
    const prev = [makeSession({ id: '1', status: 'running' })];
    const curr = [makeSession({ id: '1', status: 'dead' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0].to).toBe('dead');
  });

  it('detects stale → running transition', () => {
    const prev = [makeSession({ id: '1', status: 'stale' })];
    const curr = [makeSession({ id: '1', status: 'running' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'stale',
      to: 'running',
    });
  });

  it('ignores sessions with no status change', () => {
    const prev = [makeSession({ id: '1', status: 'running' })];
    const curr = [makeSession({ id: '1', status: 'running' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('ignores non-interesting transitions', () => {
    const prev = [makeSession({ id: '1', status: 'creating' })];
    const curr = [makeSession({ id: '1', status: 'running' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('detects multiple changes at once', () => {
    const prev = [
      makeSession({ id: '1', name: 'api-fix', status: 'running' }),
      makeSession({ id: '2', name: 'refactor', status: 'running' }),
    ];
    const curr = [
      makeSession({ id: '1', name: 'api-fix', status: 'completed' }),
      makeSession({ id: '2', name: 'refactor', status: 'dead' }),
    ];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(2);
    expect(changes[0].sessionName).toBe('api-fix');
    expect(changes[1].sessionName).toBe('refactor');
  });

  it('handles new sessions not in previous list', () => {
    const prev: Session[] = [];
    const curr = [makeSession({ id: '1', status: 'running' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('handles sessions removed from current list', () => {
    const prev = [makeSession({ id: '1', status: 'running' })];
    const curr: Session[] = [];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('handles empty lists', () => {
    const changes = detectStatusChanges([], []);

    expect(changes).toHaveLength(0);
  });
});

describe('formatStatusLabel', () => {
  it('returns completed label', () => {
    expect(
      formatStatusLabel({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'running',
        to: 'completed',
      }),
    ).toBe('my-api completed');
  });

  it('returns died label', () => {
    expect(
      formatStatusLabel({ sessionId: '1', sessionName: 'my-api', from: 'running', to: 'dead' }),
    ).toBe('my-api died');
  });

  it('returns resumed label for other transitions', () => {
    expect(
      formatStatusLabel({ sessionId: '1', sessionName: 'my-api', from: 'stale', to: 'running' }),
    ).toBe('my-api resumed');
  });
});

describe('processSessionChanges', () => {
  it('does nothing on first call (empty previousSessions)', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const current = [makeSession({ id: '1', status: 'running' })];

    const result = processSessionChanges([], current, toast, notify);

    expect(toast).not.toHaveBeenCalled();
    expect(notify).not.toHaveBeenCalled();
    expect(result).toEqual(current);
  });

  it('triggers toast and notification on status change', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const prev = [makeSession({ id: '1', status: 'running' })];
    const current = [makeSession({ id: '1', status: 'completed' })];

    processSessionChanges(prev, current, toast, notify);

    expect(toast).toHaveBeenCalledWith('my-api completed');
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ sessionName: 'my-api', to: 'completed' }),
    );
  });

  it('triggers died label for dead sessions', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const prev = [makeSession({ id: '1', status: 'running' })];
    const current = [makeSession({ id: '1', status: 'dead' })];

    processSessionChanges(prev, current, toast, notify);

    expect(toast).toHaveBeenCalledWith('my-api died');
  });

  it('triggers resumed label for stale to running', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const prev = [makeSession({ id: '1', status: 'stale' })];
    const current = [makeSession({ id: '1', status: 'running' })];

    processSessionChanges(prev, current, toast, notify);

    expect(toast).toHaveBeenCalledWith('my-api resumed');
  });

  it('returns copy of current sessions', () => {
    const current = [makeSession({ id: '1' })];
    const result = processSessionChanges([], current, vi.fn(), vi.fn());

    expect(result).toEqual(current);
    expect(result).not.toBe(current);
  });
});

describe('requestNotificationPermission', () => {
  beforeEach(() => {
    vi.stubGlobal('Notification', {
      permission: 'default',
      requestPermission: vi.fn(),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('returns true when permission is granted', async () => {
    vi.mocked(Notification.requestPermission).mockResolvedValue('granted');

    const result = await requestNotificationPermission();

    expect(result).toBe(true);
  });

  it('returns false when permission is denied', async () => {
    vi.mocked(Notification.requestPermission).mockResolvedValue('denied');

    const result = await requestNotificationPermission();

    expect(result).toBe(false);
  });

  it('returns true when permission is already granted', async () => {
    vi.stubGlobal('Notification', {
      permission: 'granted',
      requestPermission: vi.fn(),
    });

    const result = await requestNotificationPermission();

    expect(result).toBe(true);
    expect(Notification.requestPermission).not.toHaveBeenCalled();
  });

  it('returns false when Notification API is not available', async () => {
    vi.stubGlobal('Notification', undefined);

    const result = await requestNotificationPermission();

    expect(result).toBe(false);
  });
});

describe('showDesktopNotification', () => {
  let NotificationConstructor: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    NotificationConstructor = vi.fn();
    vi.stubGlobal(
      'Notification',
      Object.assign(NotificationConstructor, { permission: 'granted' }),
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('creates notification for completed session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'running',
      to: 'completed',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session completed: my-api', {
      body: 'my-api finished successfully',
    });
  });

  it('creates notification for dead session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'running',
      to: 'dead',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session died: my-api', {
      body: 'my-api has died',
    });
  });

  it('creates notification for resumed session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'stale',
      to: 'running',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session resumed: my-api', {
      body: 'my-api is now running',
    });
  });

  it('does nothing when permission is not granted', () => {
    vi.stubGlobal('Notification', Object.assign(NotificationConstructor, { permission: 'denied' }));

    showDesktopNotification({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'running',
      to: 'completed',
    });

    expect(NotificationConstructor).not.toHaveBeenCalled();
  });

  it('does nothing when Notification API is not available', () => {
    vi.stubGlobal('Notification', undefined);

    showDesktopNotification({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'running',
      to: 'completed',
    });

    // Should not throw
  });
});
