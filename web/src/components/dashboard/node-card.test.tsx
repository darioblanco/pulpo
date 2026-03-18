import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
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
    status: 'active',
    command: 'Fix',
    description: null,
    workdir: '/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,

    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

function renderNodeCard(props: {
  name: string;
  nodeInfo: NodeInfo | null;
  status: 'online' | 'offline' | 'unknown';
  sessions: Session[];
  isLocal?: boolean;
  onRefresh?: () => void;
}) {
  return render(
    <MemoryRouter>
      <NodeCard
        name={props.name}
        nodeInfo={props.nodeInfo}
        status={props.status}
        sessions={props.sessions}
        isLocal={props.isLocal}
        onRefresh={props.onRefresh ?? vi.fn()}
      />
    </MemoryRouter>,
  );
}

describe('NodeCard', () => {
  it('renders node name and info', () => {
    renderNodeCard({ name: 'mac-studio', nodeInfo, status: 'online', sessions: [] });
    expect(screen.getByText('mac-studio')).toBeInTheDocument();
    expect(screen.getByText(/macOS · arm64 · 12 cores/)).toBeInTheDocument();
  });

  it('shows local badge', () => {
    renderNodeCard({ name: 'my-node', nodeInfo, status: 'online', sessions: [], isLocal: true });
    expect(screen.getByText('local')).toBeInTheDocument();
  });

  it('shows empty message when no sessions', () => {
    renderNodeCard({ name: 'node', nodeInfo, status: 'online', sessions: [] });
    expect(screen.getByText('No active sessions on this node.')).toBeInTheDocument();
  });

  it('shows offline message', () => {
    renderNodeCard({ name: 'node', nodeInfo: null, status: 'offline', sessions: [] });
    expect(screen.getByText(/Node is offline/)).toBeInTheDocument();
  });

  it('shows unknown status message', () => {
    renderNodeCard({ name: 'node', nodeInfo: null, status: 'unknown', sessions: [] });
    expect(screen.getByText(/Node is unknown/)).toBeInTheDocument();
  });

  it('renders session cards for online node with sessions', () => {
    renderNodeCard({
      name: 'node',
      nodeInfo,
      status: 'online',
      sessions: [makeSession()],
    });
    expect(screen.getByText('my-api')).toBeInTheDocument();
  });

  it('applies opacity class for offline nodes', () => {
    renderNodeCard({ name: 'node', nodeInfo: null, status: 'offline', sessions: [] });
    expect(screen.getByTestId('node-card').className).toContain('opacity-50');
  });
});
