import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { loadConfig } from './config.js';

describe('loadConfig', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    process.env = { ...originalEnv };
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  it('throws if DISCORD_TOKEN is missing', () => {
    delete process.env.DISCORD_TOKEN;
    expect(() => loadConfig()).toThrow('DISCORD_TOKEN');
  });

  it('loads config with all env vars', () => {
    process.env.DISCORD_TOKEN = 'test-token';
    process.env.PULPOD_URL = 'http://myhost:7433';
    process.env.PULPOD_TOKEN = 'api-token';
    process.env.DISCORD_NOTIFICATION_CHANNEL_ID = 'ch123';

    const config = loadConfig();
    expect(config.discordToken).toBe('test-token');
    expect(config.pulpodUrl).toBe('http://myhost:7433');
    expect(config.pulpodToken).toBe('api-token');
    expect(config.notificationChannelId).toBe('ch123');
  });

  it('uses defaults for optional values', () => {
    process.env.DISCORD_TOKEN = 'test-token';
    delete process.env.PULPOD_URL;
    delete process.env.PULPOD_TOKEN;
    delete process.env.DISCORD_NOTIFICATION_CHANNEL_ID;

    const config = loadConfig();
    expect(config.pulpodUrl).toBe('http://localhost:7433');
    expect(config.pulpodToken).toBe('');
    expect(config.notificationChannelId).toBeUndefined();
  });

  it('treats empty DISCORD_NOTIFICATION_CHANNEL_ID as undefined', () => {
    process.env.DISCORD_TOKEN = 'test-token';
    process.env.DISCORD_NOTIFICATION_CHANNEL_ID = '';

    const config = loadConfig();
    expect(config.notificationChannelId).toBeUndefined();
  });
});
