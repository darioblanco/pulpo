import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { NodeCard } from './node-card';
import type { NodeInfo, Session } from '@/api/types';

vi.mock('@/api/client', () => ({
  killSession: vi.fn(),
  resumeSession: vi.fn(),
  getInterventionEvents: vi.fn(),
  getSessionOutput: vi.fn(),
  sendInput: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  resolveWsUrl: vi.fn().mockReturnValue('ws://localhost/test'),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.mock('@/components/session/output-view', () => ({
  OutputView: () => <div data-testid="mock-output-view" />,
}));

vi.mock('@/components/session/terminal-view', () => ({
  TerminalView: () => <div data-testid="mock-terminal-view" />,
}));

const nodeInfo: NodeInfo = {
  name: 'mac-studio',
  hostname: 'mac-studio.local',
  os: 'macOS',
  arch: 'arm64',
  cpus: 12,
  memory_mb: 65536,
  gpu: null,
};

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix',
    mode: 'interactive',
    workdir: '/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    persona: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,
    waiting_for_input: false,
    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('NodeCard', () => {
  it('renders node name and info', () => {
    render(
      <NodeCard
        name="mac-studio"
        nodeInfo={nodeInfo}
        status="online"
        sessions={[]}
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.getByText('mac-studio')).toBeInTheDocument();
    expect(screen.getByText(/macOS · arm64 · 12 cores/)).toBeInTheDocument();
  });

  it('shows local badge', () => {
    render(
      <NodeCard
        name="my-node"
        nodeInfo={nodeInfo}
        status="online"
        sessions={[]}
        isLocal
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.getByText('local')).toBeInTheDocument();
  });

  it('shows empty message when no sessions', () => {
    render(
      <NodeCard
        name="node"
        nodeInfo={nodeInfo}
        status="online"
        sessions={[]}
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.getByText('No active sessions on this node.')).toBeInTheDocument();
  });

  it('shows offline message', () => {
    render(
      <NodeCard name="node" nodeInfo={null} status="offline" sessions={[]} onRefresh={vi.fn()} />,
    );
    expect(screen.getByText(/Node is offline/)).toBeInTheDocument();
  });

  it('shows unknown status message', () => {
    render(
      <NodeCard name="node" nodeInfo={null} status="unknown" sessions={[]} onRefresh={vi.fn()} />,
    );
    expect(screen.getByText(/Node is unknown/)).toBeInTheDocument();
  });

  it('renders session cards for online node with sessions', () => {
    render(
      <NodeCard
        name="node"
        nodeInfo={nodeInfo}
        status="online"
        sessions={[makeSession()]}
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.getByText('my-api')).toBeInTheDocument();
  });

  it('applies opacity class for offline nodes', () => {
    render(
      <NodeCard name="node" nodeInfo={null} status="offline" sessions={[]} onRefresh={vi.fn()} />,
    );
    expect(screen.getByTestId('node-card').className).toContain('opacity-50');
  });
});
