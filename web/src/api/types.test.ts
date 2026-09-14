import { describe, it, expect } from 'vitest';
import type { Session, SessionStatus, CreateSessionRequest, ConfigResponse } from './types';

describe('types', () => {
  it('SessionStatus covers the five-state model (ADR 0009)', () => {
    const statuses: SessionStatus[] = ['starting', 'working', 'waiting', 'done', 'lost'];
    expect(statuses).toHaveLength(5);
  });

  it('Session type carries status_reason and optional exit_code', () => {
    const session: Session = {
      id: 'sess-1',
      name: 'test',
      status: 'done',
      status_reason: 'exited',
      exit_code: 0,
      command: 'claude code',
      description: null,
      workdir: '/repo',
      metadata: null,
      ink: null,
      intervention_reason: null,
      intervention_at: null,
      last_output_at: null,
      created_at: '2026-01-01T00:00:00Z',
    };
    expect(session.status_reason).toBe('exited');
    expect(session.exit_code).toBe(0);
  });

  it('Session type has command and description fields', () => {
    const session: Session = {
      id: 'sess-1',
      name: 'test',
      status: 'working',
      status_reason: null,
      command: 'claude code',
      description: 'Fix the bug',
      workdir: '/repo',
      metadata: null,
      ink: null,
      intervention_reason: null,
      intervention_at: null,
      last_output_at: null,
      created_at: '2026-01-01T00:00:00Z',
    };
    expect(session.command).toBe('claude code');
    expect(session.description).toBe('Fix the bug');
  });

  it('Session type has optional harness fields', () => {
    const session: Session = {
      id: 'sess-1',
      name: 'test',
      status: 'waiting',
      status_reason: 'needs_input:permission',
      command: 'claude -p fix',
      description: null,
      workdir: '/repo',
      metadata: null,
      ink: null,
      intervention_reason: null,
      intervention_at: null,
      last_output_at: null,
      created_at: '2026-01-01T00:00:00Z',
      harness: 'claude',
      harness_session_id: 'sid-123',
      harness_last_event_at: '2026-01-01T00:00:05Z',
    };
    expect(session.harness).toBe('claude');
    expect(session.harness_session_id).toBe('sid-123');
    expect(session.harness_last_event_at).toBe('2026-01-01T00:00:05Z');
  });

  it('CreateSessionRequest has command and description fields', () => {
    const req: CreateSessionRequest = {
      name: 'my-session',
      workdir: '/repo',
      command: 'claude code',
      description: 'Fix stuff',
    };
    expect(req.command).toBe('claude code');
    expect(req.description).toBe('Fix stuff');
  });

  it('ConfigResponse has no guards field', () => {
    const config: ConfigResponse = {
      node: {
        name: 'test',
        port: 7433,
        data_dir: '~/.pulpo/data',
        bind: 'local',
      },
      watchdog: {
        enabled: true,
        check_interval_secs: 30,
        idle_timeout_secs: 300,
        idle_action: 'pause',
        idle_threshold_secs: 60,
        extra_waiting_patterns: [],
      },
      notifications: { webhooks: [] },
    };
    expect(config.node.name).toBe('test');
    // Verify guards is not a property
    expect('guards' in config).toBe(false);
  });
});
