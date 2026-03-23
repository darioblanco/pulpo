import { describe, it, expect } from 'vitest';
import { sessionEmbed, eventEmbed, sessionListEmbed, inkListEmbed } from './embed.js';
import type { Session, SessionEvent } from '../api/pulpod.js';

function mockSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'abc-123',
    name: 'my-session',
    workdir: '/code/repo',
    command: 'claude "Fix the tests"',
    description: 'Fix the tests',
    status: 'active',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function mockEvent(overrides: Partial<SessionEvent> = {}): SessionEvent {
  return {
    session_id: 'abc-123',
    session_name: 'my-session',
    status: 'active',
    node_name: 'node-1',
    timestamp: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('sessionEmbed', () => {
  it('creates embed with basic fields', () => {
    const embed = sessionEmbed(mockSession());
    const json = embed.toJSON();

    expect(json.title).toContain('my-session');
    expect(json.color).toBe(0x2ecc71);
    expect(json.fields).toBeDefined();
    expect(json.fields!.some((f) => f.name === 'Status' && f.value === 'active')).toBe(true);
    expect(json.fields!.some((f) => f.name === 'ID')).toBe(true);
    expect(json.fields!.some((f) => f.name === 'Command')).toBe(true);
  });

  it('includes ink when present', () => {
    const embed = sessionEmbed(mockSession({ ink: 'coder' }));
    const json = embed.toJSON();
    expect(json.fields!.some((f) => f.name === 'Ink' && f.value === 'coder')).toBe(true);
  });

  it('includes description when present', () => {
    const embed = sessionEmbed(mockSession({ description: 'Fix the tests' }));
    const json = embed.toJSON();
    expect(json.fields!.some((f) => f.name === 'Description' && f.value === 'Fix the tests')).toBe(
      true,
    );
  });

  it('truncates long commands', () => {
    const longCommand = 'a'.repeat(300);
    const embed = sessionEmbed(mockSession({ command: longCommand }));
    const json = embed.toJSON();
    const commandField = json.fields!.find((f) => f.name === 'Command');
    expect(commandField!.value.length).toBeLessThan(215);
    expect(commandField!.value).toContain('...');
  });

  it('uses correct colors for different statuses', () => {
    expect(sessionEmbed(mockSession({ status: 'active' })).toJSON().color).toBe(0x2ecc71);
    expect(sessionEmbed(mockSession({ status: 'ready' })).toJSON().color).toBe(0x3498db);
    expect(sessionEmbed(mockSession({ status: 'stopped' })).toJSON().color).toBe(0xe74c3c);
    expect(sessionEmbed(mockSession({ status: 'lost' })).toJSON().color).toBe(0xe67e22);
    expect(sessionEmbed(mockSession({ status: 'idle' })).toJSON().color).toBe(0xf59e0b);
    expect(sessionEmbed(mockSession({ status: 'creating' })).toJSON().color).toBe(0x95a5a6);
    expect(sessionEmbed(mockSession({ status: 'unknown' })).toJSON().color).toBe(0x95a5a6);
  });
});

describe('eventEmbed', () => {
  it('creates embed with basic event fields', () => {
    const embed = eventEmbed(mockEvent());
    const json = embed.toJSON();

    expect(json.title).toContain('my-session');
    expect(json.description).toContain('abc-123');
    expect(json.description).toContain('active');
    expect(json.color).toBe(0x2ecc71);
    expect(json.fields!.some((f) => f.name === 'Status' && f.value === 'active')).toBe(true);
    expect(json.fields!.some((f) => f.name === 'Node' && f.value === 'node-1')).toBe(true);
  });

  it('includes previous status when present', () => {
    const embed = eventEmbed(mockEvent({ previous_status: 'creating' }));
    const json = embed.toJSON();
    expect(json.fields!.some((f) => f.name === 'Previous' && f.value === 'creating')).toBe(true);
  });

  it('includes output snippet when present', () => {
    const embed = eventEmbed(mockEvent({ output_snippet: 'hello world' }));
    const json = embed.toJSON();
    const outputField = json.fields!.find((f) => f.name === 'Output');
    expect(outputField).toBeDefined();
    expect(outputField!.value).toContain('hello world');
    expect(outputField!.inline).toBe(false);
  });

  it('truncates long output snippets', () => {
    const longOutput = 'x'.repeat(1500);
    const embed = eventEmbed(mockEvent({ output_snippet: longOutput }));
    const json = embed.toJSON();
    const outputField = json.fields!.find((f) => f.name === 'Output');
    expect(outputField!.value.length).toBeLessThan(1020);
  });

  it('uses stopped color for stopped events', () => {
    const embed = eventEmbed(mockEvent({ status: 'stopped' }));
    expect(embed.toJSON().color).toBe(0xe74c3c);
  });
});

describe('inkListEmbed', () => {
  it('shows empty message when no inks', () => {
    const embed = inkListEmbed({});
    const json = embed.toJSON();
    expect(json.title).toBe('Inks');
    expect(json.description).toContain('No inks configured');
  });

  it('lists inks with key fields', () => {
    const embed = inkListEmbed({
      coder: { description: 'Autonomous coder', command: 'claude --dangerously-skip-permissions' },
      reviewer: { description: null, command: 'codex' },
    });
    const json = embed.toJSON();
    expect(json.description).toContain('coder');
    expect(json.description).toContain('claude');
    expect(json.description).toContain('reviewer');
    expect(json.description).toContain('codex');
  });

  it('uses purple color', () => {
    const embed = inkListEmbed({});
    expect(embed.toJSON().color).toBe(0x9b59b6);
  });
});

describe('sessionListEmbed', () => {
  it('shows empty message when no sessions', () => {
    const embed = sessionListEmbed([]);
    const json = embed.toJSON();
    expect(json.title).toBe('Sessions');
    expect(json.description).toContain('No sessions found');
  });

  it('lists sessions with status emojis', () => {
    const sessions = [
      mockSession({ name: 'session-1', status: 'active' }),
      mockSession({ name: 'session-2', status: 'ready' }),
    ];
    const embed = sessionListEmbed(sessions);
    const json = embed.toJSON();
    expect(json.description).toContain('session-1');
    expect(json.description).toContain('session-2');
    expect(json.description).toContain('active');
    expect(json.description).toContain('ready');
  });

  it('truncates to 25 sessions and shows footer', () => {
    const sessions = Array.from({ length: 30 }, (_, i) =>
      mockSession({ name: `session-${i}`, id: `id-${i}` }),
    );
    const embed = sessionListEmbed(sessions);
    const json = embed.toJSON();
    expect(json.footer?.text).toContain('30');
    // Should not contain session-25 through session-29
    expect(json.description).not.toContain('session-29');
  });
});
