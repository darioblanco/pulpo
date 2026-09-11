import { describe, it, expect } from 'vitest';
import type {
  Session,
  CreateSessionRequest,
  ConfigResponse,
  VapidPublicKeyResponse,
  PushSubscriptionRequest,
} from './types';

describe('types', () => {
  it('Session type has command and description fields', () => {
    const session: Session = {
      id: 'sess-1',
      name: 'test',
      status: 'active',
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
      status: 'idle',
      command: 'claude -p fix',
      description: null,
      workdir: '/repo',
      metadata: { needs_input: 'permission' },
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
        tag: null,
      },
      watchdog: {
        enabled: true,
        memory_threshold: 85,
        check_interval_secs: 30,
        breach_count: 3,
        idle_timeout_secs: 300,
        idle_action: 'pause',
        ready_ttl_secs: 0,
      },
      notifications: { webhooks: [] },
    };
    expect(config.node.name).toBe('test');
    // Verify guards is not a property
    expect('guards' in config).toBe(false);
  });

  it('VapidPublicKeyResponse has public_key field', () => {
    const resp: VapidPublicKeyResponse = {
      public_key: 'BNhJo...',
    };
    expect(resp.public_key).toBe('BNhJo...');
  });

  it('PushSubscriptionRequest has endpoint and keys', () => {
    const req: PushSubscriptionRequest = {
      endpoint: 'https://fcm.googleapis.com/fcm/send/abc',
      keys: {
        p256dh: 'key-data',
        auth: 'auth-data',
      },
    };
    expect(req.endpoint).toBe('https://fcm.googleapis.com/fcm/send/abc');
    expect(req.keys.p256dh).toBe('key-data');
    expect(req.keys.auth).toBe('auth-data');
  });
});
