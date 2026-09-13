import { describe, it, expect } from 'vitest';
import { detectStatusChanges } from './notifications';
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
