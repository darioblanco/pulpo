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
    status: 'active',
    prompt: 'Fix the bug',
    mode: 'interactive',
    workdir: '/home/user/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    ink: null,
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
  it('detects active → finished transition', () => {
    const prev = [makeSession({ id: '1', status: 'active' })];
    const curr = [makeSession({ id: '1', status: 'finished' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'finished',
    });
  });

  it('detects active → killed transition', () => {
    const prev = [makeSession({ id: '1', status: 'active' })];
    const curr = [makeSession({ id: '1', status: 'killed' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0].to).toBe('killed');
  });

  it('detects lost → active transition', () => {
    const prev = [makeSession({ id: '1', status: 'lost' })];
    const curr = [makeSession({ id: '1', status: 'active' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'lost',
      to: 'active',
    });
  });

  it('ignores sessions with no status change', () => {
    const prev = [makeSession({ id: '1', status: 'active' })];
    const curr = [makeSession({ id: '1', status: 'active' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('ignores non-interesting transitions', () => {
    const prev = [makeSession({ id: '1', status: 'creating' })];
    const curr = [makeSession({ id: '1', status: 'active' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('detects multiple changes at once', () => {
    const prev = [
      makeSession({ id: '1', name: 'api-fix', status: 'active' }),
      makeSession({ id: '2', name: 'refactor', status: 'active' }),
    ];
    const curr = [
      makeSession({ id: '1', name: 'api-fix', status: 'finished' }),
      makeSession({ id: '2', name: 'refactor', status: 'killed' }),
    ];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(2);
    expect(changes[0].sessionName).toBe('api-fix');
    expect(changes[1].sessionName).toBe('refactor');
  });

  it('handles new sessions not in previous list', () => {
    const prev: Session[] = [];
    const curr = [makeSession({ id: '1', status: 'active' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('handles sessions removed from current list', () => {
    const prev = [makeSession({ id: '1', status: 'active' })];
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
  it('returns finished label', () => {
    expect(
      formatStatusLabel({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'active',
        to: 'finished',
      }),
    ).toBe('my-api finished');
  });

  it('returns killed label', () => {
    expect(
      formatStatusLabel({ sessionId: '1', sessionName: 'my-api', from: 'active', to: 'killed' }),
    ).toBe('my-api killed');
  });

  it('returns resumed label for other transitions', () => {
    expect(
      formatStatusLabel({ sessionId: '1', sessionName: 'my-api', from: 'lost', to: 'active' }),
    ).toBe('my-api resumed');
  });
});

describe('processSessionChanges', () => {
  it('does nothing on first call (empty previousSessions)', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const current = [makeSession({ id: '1', status: 'active' })];

    const result = processSessionChanges([], current, toast, notify);

    expect(toast).not.toHaveBeenCalled();
    expect(notify).not.toHaveBeenCalled();
    expect(result).toEqual(current);
  });

  it('triggers toast and notification on status change', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const prev = [makeSession({ id: '1', status: 'active' })];
    const current = [makeSession({ id: '1', status: 'finished' })];

    processSessionChanges(prev, current, toast, notify);

    expect(toast).toHaveBeenCalledWith('my-api finished');
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ sessionName: 'my-api', to: 'finished' }),
    );
  });

  it('triggers killed label for killed sessions', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const prev = [makeSession({ id: '1', status: 'active' })];
    const current = [makeSession({ id: '1', status: 'killed' })];

    processSessionChanges(prev, current, toast, notify);

    expect(toast).toHaveBeenCalledWith('my-api killed');
  });

  it('triggers resumed label for lost to active', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const prev = [makeSession({ id: '1', status: 'lost' })];
    const current = [makeSession({ id: '1', status: 'active' })];

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

  it('creates notification for finished session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'finished',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session finished: my-api', {
      body: 'my-api finished successfully',
    });
  });

  it('creates notification for killed session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'killed',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session killed: my-api', {
      body: 'my-api has been killed',
    });
  });

  it('creates notification for resumed session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'lost',
      to: 'active',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session resumed: my-api', {
      body: 'my-api is now active',
    });
  });

  it('does nothing when permission is not granted', () => {
    vi.stubGlobal('Notification', Object.assign(NotificationConstructor, { permission: 'denied' }));

    showDesktopNotification({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'finished',
    });

    expect(NotificationConstructor).not.toHaveBeenCalled();
  });

  it('does nothing when Notification API is not available', () => {
    vi.stubGlobal('Notification', undefined);

    showDesktopNotification({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'finished',
    });

    // Should not throw
  });
});
