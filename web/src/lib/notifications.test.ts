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
    status: 'active',
    command: 'Fix the bug',
    description: null,
    workdir: '/home/user/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,

    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('detectStatusChanges', () => {
  it('detects active → ready transition', () => {
    const prev = [makeSession({ id: '1', status: 'active' })];
    const curr = [makeSession({ id: '1', status: 'ready' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual(
      expect.objectContaining({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'active',
        to: 'ready',
      }),
    );
  });

  it('detects active → stopped transition', () => {
    const prev = [makeSession({ id: '1', status: 'active' })];
    const curr = [makeSession({ id: '1', status: 'stopped' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0].to).toBe('stopped');
  });

  it('detects lost → active transition', () => {
    const prev = [makeSession({ id: '1', status: 'lost' })];
    const curr = [makeSession({ id: '1', status: 'active' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual(
      expect.objectContaining({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'lost',
        to: 'active',
      }),
    );
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
      makeSession({ id: '1', name: 'api-fix', status: 'ready' }),
      makeSession({ id: '2', name: 'refactor', status: 'stopped' }),
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
  it('returns ready label', () => {
    expect(
      formatStatusLabel({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'active',
        to: 'ready',
      }),
    ).toBe('my-api ready');
  });

  it('returns stopped label', () => {
    expect(
      formatStatusLabel({ sessionId: '1', sessionName: 'my-api', from: 'active', to: 'stopped' }),
    ).toBe('my-api stopped');
  });

  it('returns resumed label for other transitions', () => {
    expect(
      formatStatusLabel({ sessionId: '1', sessionName: 'my-api', from: 'lost', to: 'active' }),
    ).toBe('my-api resumed');
  });

  it('includes branch and changes when available', () => {
    expect(
      formatStatusLabel({
        sessionId: '1',
        sessionName: 'portal',
        from: 'active',
        to: 'ready',
        gitBranch: 'main',
        gitInsertions: 42,
        gitDeletions: 7,
        gitFilesChanged: 3,
      }),
    ).toBe('portal ready (+42/-7, 3 files on main)');
  });

  it('includes branch only when no changes', () => {
    expect(
      formatStatusLabel({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'active',
        to: 'ready',
        gitBranch: 'fix-auth',
      }),
    ).toBe('my-api ready on fix-auth');
  });

  it('includes error status when available', () => {
    expect(
      formatStatusLabel({
        sessionId: '1',
        sessionName: 'test',
        from: 'active',
        to: 'stopped',
        errorStatus: 'Compile error',
      }),
    ).toBe('test stopped (Compile error)');
  });

  it('prefers error status over branch info', () => {
    expect(
      formatStatusLabel({
        sessionId: '1',
        sessionName: 'test',
        from: 'active',
        to: 'stopped',
        errorStatus: 'Compile error',
        gitBranch: 'main',
      }),
    ).toBe('test stopped (Compile error)');
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
    const current = [makeSession({ id: '1', status: 'ready' })];

    processSessionChanges(prev, current, toast, notify);

    expect(toast).toHaveBeenCalledWith('my-api ready');
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ sessionName: 'my-api', to: 'ready' }),
    );
  });

  it('triggers stopped label for stopped sessions', () => {
    const toast = vi.fn();
    const notify = vi.fn();
    const prev = [makeSession({ id: '1', status: 'active' })];
    const current = [makeSession({ id: '1', status: 'stopped' })];

    processSessionChanges(prev, current, toast, notify);

    expect(toast).toHaveBeenCalledWith('my-api stopped');
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

  it('creates notification for ready session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'ready',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session ready: my-api', {
      body: 'my-api is ready',
    });
  });

  it('creates notification for stopped session', () => {
    const change: StatusChange = {
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'stopped',
    };

    showDesktopNotification(change);

    expect(NotificationConstructor).toHaveBeenCalledWith('Session stopped: my-api', {
      body: 'my-api has been stopped',
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
      to: 'ready',
    });

    expect(NotificationConstructor).not.toHaveBeenCalled();
  });

  it('does nothing when Notification API is not available', () => {
    vi.stubGlobal('Notification', undefined);

    showDesktopNotification({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'ready',
    });

    // Should not throw
  });

  it('includes enrichment info in ready notification body', () => {
    showDesktopNotification({
      sessionId: '1',
      sessionName: 'portal',
      from: 'active',
      to: 'ready',
      gitBranch: 'main',
      gitInsertions: 42,
      gitDeletions: 7,
      gitFilesChanged: 3,
    });

    expect(NotificationConstructor).toHaveBeenCalledWith('Session ready: portal', {
      body: 'portal is ready — +42/-7 (3 files) on main',
    });
  });

  it('includes branch only in notification body', () => {
    showDesktopNotification({
      sessionId: '1',
      sessionName: 'my-api',
      from: 'active',
      to: 'ready',
      gitBranch: 'fix-auth',
    });

    expect(NotificationConstructor).toHaveBeenCalledWith('Session ready: my-api', {
      body: 'my-api is ready — on branch fix-auth',
    });
  });

  it('includes error status in stopped notification body', () => {
    showDesktopNotification({
      sessionId: '1',
      sessionName: 'test',
      from: 'active',
      to: 'stopped',
      errorStatus: 'Compile error',
    });

    expect(NotificationConstructor).toHaveBeenCalledWith('Session stopped: test', {
      body: 'test has been stopped — Error: Compile error',
    });
  });
});
