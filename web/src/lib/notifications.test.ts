import { describe, it, expect } from 'vitest';
import { detectStatusChanges } from './notifications';
import type { Session } from '@/api/types';

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    status: 'working',
    status_reason: null,
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
  it('detects working → done (clean exit) transition', () => {
    const prev = [makeSession({ id: '1', status: 'working' })];
    const curr = [makeSession({ id: '1', status: 'done', status_reason: 'exited' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual(
      expect.objectContaining({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'working',
        to: 'done',
        toReason: 'exited',
      }),
    );
  });

  it('detects working → done (forced stop) transition', () => {
    const prev = [makeSession({ id: '1', status: 'working' })];
    const curr = [makeSession({ id: '1', status: 'done', status_reason: 'stopped' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0].to).toBe('done');
    expect(changes[0].toReason).toBe('stopped');
  });

  it('detects lost → working transition', () => {
    const prev = [makeSession({ id: '1', status: 'lost' })];
    const curr = [makeSession({ id: '1', status: 'working' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(1);
    expect(changes[0]).toEqual(
      expect.objectContaining({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'lost',
        to: 'working',
      }),
    );
  });

  it('ignores sessions with no status change', () => {
    const prev = [makeSession({ id: '1', status: 'working' })];
    const curr = [makeSession({ id: '1', status: 'working' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('ignores non-interesting transitions', () => {
    const prev = [makeSession({ id: '1', status: 'starting' })];
    const curr = [makeSession({ id: '1', status: 'working' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('detects multiple changes at once', () => {
    const prev = [
      makeSession({ id: '1', name: 'api-fix', status: 'working' }),
      makeSession({ id: '2', name: 'refactor', status: 'working' }),
    ];
    const curr = [
      makeSession({ id: '1', name: 'api-fix', status: 'done', status_reason: 'exited' }),
      makeSession({ id: '2', name: 'refactor', status: 'done', status_reason: 'stopped' }),
    ];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(2);
    expect(changes[0].sessionName).toBe('api-fix');
    expect(changes[1].sessionName).toBe('refactor');
  });

  it('handles new sessions not in previous list', () => {
    const prev: Session[] = [];
    const curr = [makeSession({ id: '1', status: 'working' })];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('handles sessions removed from current list', () => {
    const prev = [makeSession({ id: '1', status: 'working' })];
    const curr: Session[] = [];

    const changes = detectStatusChanges(prev, curr);

    expect(changes).toHaveLength(0);
  });

  it('handles empty lists', () => {
    const changes = detectStatusChanges([], []);

    expect(changes).toHaveLength(0);
  });
});
