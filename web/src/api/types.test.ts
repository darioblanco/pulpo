import { describe, it, expect } from 'vitest';
import type {
  Session,
  InkConfig,
  CreateSessionRequest,
  ConfigResponse,
  VapidPublicKeyResponse,
  PushSubscriptionRequest,
  SecretEntry,
  SecretListResponse,
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

  it('InkConfig has description and command fields', () => {
    const ink: InkConfig = {
      description: 'Code reviewer',
      command: 'claude code --model opus-4',
    };
    expect(ink.description).toBe('Code reviewer');
    expect(ink.command).toBe('claude code --model opus-4');
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
        seed: null,
        discovery_interval_secs: 60,
      },
      peers: {},
      watchdog: {
        enabled: true,
        memory_threshold: 85,
        check_interval_secs: 30,
        breach_count: 3,
        idle_timeout_secs: 300,
        idle_action: 'pause',
        ready_ttl_secs: 0,
        adopt_tmux: true,
      },
      notifications: { discord: null, webhooks: [] },
      inks: {},
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

  it('SecretEntry has name and created_at', () => {
    const entry: SecretEntry = {
      name: 'GITHUB_TOKEN',
      created_at: '2026-01-01T00:00:00Z',
    };
    expect(entry.name).toBe('GITHUB_TOKEN');
    expect(entry.created_at).toBe('2026-01-01T00:00:00Z');
  });

  it('SecretListResponse contains secrets array', () => {
    const resp: SecretListResponse = {
      secrets: [{ name: 'TOKEN', created_at: '2026-01-01T00:00:00Z' }],
    };
    expect(resp.secrets).toHaveLength(1);
    expect(resp.secrets[0].name).toBe('TOKEN');
  });
});
