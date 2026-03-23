import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { NodeCard } from './node-card';
import type { NodeInfo, Session } from '@/api/types';

vi.mock('@/api/client', () => ({
  stopSession: vi.fn(),
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
  address?: string;
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
        address={props.address}
        onRefresh={props.onRefresh ?? vi.fn()}
      />
    </MemoryRouter>,
  );
}

describe('NodeCard', () => {
  it('renders node name', () => {
    renderNodeCard({ name: 'mac-studio', nodeInfo, status: 'online', sessions: [] });
    expect(screen.getByText('mac-studio')).toBeInTheDocument();
  });

  it('renders node info bar with hardware details', () => {
    renderNodeCard({ name: 'mac-studio', nodeInfo, status: 'online', sessions: [] });
    const infoBar = screen.getByTestId('node-info-bar');
    expect(infoBar).toBeInTheDocument();
    expect(infoBar).toHaveTextContent('mac-studio.local');
    expect(infoBar).toHaveTextContent('macOS arm64');
    expect(infoBar).toHaveTextContent('12 CPU');
    expect(infoBar).toHaveTextContent('64 GB');
  });

  it('shows GPU when present', () => {
    const withGpu: NodeInfo = { ...nodeInfo, gpu: 'NVIDIA RTX 4090' };
    renderNodeCard({ name: 'gpu-node', nodeInfo: withGpu, status: 'online', sessions: [] });
    expect(screen.getByText('NVIDIA RTX 4090')).toBeInTheDocument();
  });

  it('hides GPU when null', () => {
    renderNodeCard({ name: 'mac-studio', nodeInfo, status: 'online', sessions: [] });
    expect(screen.queryByText('NVIDIA')).not.toBeInTheDocument();
  });

  it('shows address when provided', () => {
    renderNodeCard({
      name: 'mac-studio',
      nodeInfo,
      status: 'online',
      sessions: [],
      address: '100.64.0.1:7433',
    });
    expect(screen.getByText('100.64.0.1:7433')).toBeInTheDocument();
  });

  it('hides address when not provided', () => {
    renderNodeCard({ name: 'mac-studio', nodeInfo, status: 'online', sessions: [] });
    expect(screen.queryByText(/:\d{4}$/)).not.toBeInTheDocument();
  });

  it('does not render info bar when nodeInfo is null', () => {
    renderNodeCard({ name: 'node', nodeInfo: null, status: 'offline', sessions: [] });
    expect(screen.queryByTestId('node-info-bar')).not.toBeInTheDocument();
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

  it('formats memory in MB for small values', () => {
    const smallMem: NodeInfo = { ...nodeInfo, memory_mb: 512 };
    renderNodeCard({ name: 'node', nodeInfo: smallMem, status: 'online', sessions: [] });
    expect(screen.getByTestId('node-info-bar')).toHaveTextContent('512 MB');
  });
});
