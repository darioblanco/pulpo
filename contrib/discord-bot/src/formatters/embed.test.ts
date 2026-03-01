import { describe, it, expect } from 'vitest';
import { sessionEmbed, eventEmbed, sessionListEmbed, personaListEmbed } from './embed.js';
import type { Session, SessionEvent } from '../api/pulpod.js';

function mockSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'abc-123',
    name: 'my-session',
    workdir: '/code/repo',
    provider: 'claude',
    prompt: 'Fix the tests',
    status: 'running',
    mode: 'autonomous',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function mockEvent(overrides: Partial<SessionEvent> = {}): SessionEvent {
  return {
    session_id: 'abc-123',
    session_name: 'my-session',
    status: 'running',
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
    expect(json.fields!.some((f) => f.name === 'Status' && f.value === 'running')).toBe(true);
    expect(json.fields!.some((f) => f.name === 'Provider' && f.value === 'claude')).toBe(true);
    expect(json.fields!.some((f) => f.name === 'ID')).toBe(true);
    expect(json.fields!.some((f) => f.name === 'Prompt')).toBe(true);
  });

  it('includes model when present', () => {
    const embed = sessionEmbed(mockSession({ model: 'opus' }));
    const json = embed.toJSON();
    expect(json.fields!.some((f) => f.name === 'Model' && f.value === 'opus')).toBe(true);
  });

  it('includes persona when present', () => {
    const embed = sessionEmbed(mockSession({ persona: 'coder' }));
    const json = embed.toJSON();
    expect(json.fields!.some((f) => f.name === 'Persona' && f.value === 'coder')).toBe(true);
  });

  it('truncates long prompts', () => {
    const longPrompt = 'a'.repeat(300);
    const embed = sessionEmbed(mockSession({ prompt: longPrompt }));
    const json = embed.toJSON();
    const promptField = json.fields!.find((f) => f.name === 'Prompt');
    expect(promptField!.value.length).toBeLessThan(210);
    expect(promptField!.value.endsWith('...')).toBe(true);
  });

  it('uses correct colors for different statuses', () => {
    expect(sessionEmbed(mockSession({ status: 'running' })).toJSON().color).toBe(0x2ecc71);
    expect(sessionEmbed(mockSession({ status: 'completed' })).toJSON().color).toBe(0x3498db);
    expect(sessionEmbed(mockSession({ status: 'dead' })).toJSON().color).toBe(0xe74c3c);
    expect(sessionEmbed(mockSession({ status: 'stale' })).toJSON().color).toBe(0xe67e22);
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
    expect(json.description).toContain('running');
    expect(json.color).toBe(0x2ecc71);
    expect(json.fields!.some((f) => f.name === 'Status' && f.value === 'running')).toBe(true);
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

  it('uses dead color for dead events', () => {
    const embed = eventEmbed(mockEvent({ status: 'dead' }));
    expect(embed.toJSON().color).toBe(0xe74c3c);
  });
});

describe('personaListEmbed', () => {
  it('shows empty message when no personas', () => {
    const embed = personaListEmbed({});
    const json = embed.toJSON();
    expect(json.title).toBe('Personas');
    expect(json.description).toContain('No personas configured');
  });

  it('lists personas with key fields', () => {
    const embed = personaListEmbed({
      coder: { provider: 'claude', model: 'opus', mode: 'autonomous', guard_preset: 'strict' },
      reviewer: { provider: 'codex', model: 'codex-mini' },
    });
    const json = embed.toJSON();
    expect(json.description).toContain('coder');
    expect(json.description).toContain('claude');
    expect(json.description).toContain('opus');
    expect(json.description).toContain('reviewer');
    expect(json.description).toContain('codex-mini');
  });

  it('uses purple color', () => {
    const embed = personaListEmbed({});
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
      mockSession({ name: 'session-1', status: 'running', provider: 'claude' }),
      mockSession({ name: 'session-2', status: 'completed', provider: 'codex' }),
    ];
    const embed = sessionListEmbed(sessions);
    const json = embed.toJSON();
    expect(json.description).toContain('session-1');
    expect(json.description).toContain('session-2');
    expect(json.description).toContain('running');
    expect(json.description).toContain('completed');
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
