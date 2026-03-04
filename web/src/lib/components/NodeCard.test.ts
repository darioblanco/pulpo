import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render, screen } from '@testing-library/svelte';
import NodeCard from './NodeCard.svelte';
import type { NodeInfo, Session } from '$lib/api';

function makeNodeInfo(overrides: Partial<NodeInfo> = {}): NodeInfo {
  return {
    name: 'mac-mini',
    hostname: 'mac-mini.local',
    os: 'macos',
    arch: 'aarch64',
    cpus: 10,
    memory_mb: 16384,
    gpu: null,
    ...overrides,
  };
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix the bug',
    mode: 'interactive',
    workdir: '/home/user/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    persona: null,
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

afterEach(cleanup);

describe('NodeCard', () => {
  it('renders node name and session details for online node', () => {
    const sessions = [makeSession(), makeSession({ id: 'sess-2', name: 'docs' })];
    render(NodeCard, {
      props: {
        name: 'mac-mini',
        nodeInfo: makeNodeInfo(),
        status: 'online' as const,
        sessions,
        onrefresh: vi.fn(),
      },
    });

    expect(screen.getByText('mac-mini')).toBeTruthy();
    expect(screen.getByText('2 sessions')).toBeTruthy();
    expect(screen.getByText('macos · aarch64 · 10 cores')).toBeTruthy();
  });

  it('shows offline message for offline node', () => {
    render(NodeCard, {
      props: {
        name: 'win-pc',
        nodeInfo: null,
        status: 'offline' as const,
        sessions: [],
        onrefresh: vi.fn(),
      },
    });

    expect(screen.getByText('Node is offline — cannot fetch sessions.')).toBeTruthy();
  });

  it('shows unknown message for unknown status', () => {
    render(NodeCard, {
      props: {
        name: 'mystery',
        nodeInfo: null,
        status: 'unknown' as const,
        sessions: [],
        onrefresh: vi.fn(),
      },
    });

    expect(screen.getByText('Node is unknown — cannot fetch sessions.')).toBeTruthy();
  });

  it('shows local badge when isLocal=true', () => {
    render(NodeCard, {
      props: {
        name: 'mac-mini',
        nodeInfo: makeNodeInfo(),
        status: 'online' as const,
        sessions: [],
        isLocal: true,
        onrefresh: vi.fn(),
      },
    });

    expect(screen.getByText('local')).toBeTruthy();
  });

  it('shows "No active sessions" for online node with no sessions', () => {
    render(NodeCard, {
      props: {
        name: 'mac-mini',
        nodeInfo: makeNodeInfo(),
        status: 'online' as const,
        sessions: [],
        onrefresh: vi.fn(),
      },
    });

    expect(screen.getByText('No active sessions on this node.')).toBeTruthy();
  });

  it('pluralizes session count correctly for 1 session', () => {
    render(NodeCard, {
      props: {
        name: 'mac-mini',
        nodeInfo: makeNodeInfo(),
        status: 'online' as const,
        sessions: [makeSession()],
        onrefresh: vi.fn(),
      },
    });

    expect(screen.getByText('1 session')).toBeTruthy();
  });
});
