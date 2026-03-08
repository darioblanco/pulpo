import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { OceanCanvas } from './ocean-canvas';
import type { Session, NodeInfo, PeerInfo } from '@/api/types';

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'api-fix',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix the auth bug',
    mode: 'autonomous',
    workdir: '/tmp/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    ink: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,
    waiting_for_input: false,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function makeNode(overrides: Partial<NodeInfo> = {}): NodeInfo {
  return {
    name: 'mac-studio',
    hostname: 'mac-studio.local',
    os: 'macos',
    arch: 'aarch64',
    cpus: 12,
    memory_mb: 32768,
    gpu: null,
    ...overrides,
  };
}

describe('OceanCanvas', () => {
  it('renders the ocean SVG', () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    expect(screen.getByTestId('ocean-canvas')).toBeInTheDocument();
  });

  it('renders local node island', () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    expect(screen.getByText('mac-studio')).toBeInTheDocument();
  });

  it('renders octopuses for local sessions', () => {
    const sessions = [makeSession({ name: 'worker-1' }), makeSession({ name: 'worker-2' })];
    render(
      <OceanCanvas localNode={makeNode()} localSessions={sessions} peers={[]} peerSessions={{}} />,
    );
    expect(screen.getByTestId('octopus-worker-1')).toBeInTheDocument();
    expect(screen.getByTestId('octopus-worker-2')).toBeInTheDocument();
  });

  it('renders peer islands', () => {
    const peers: PeerInfo[] = [
      {
        name: 'linux-server',
        address: '100.64.1.2:7433',
        status: 'online',
        node_info: makeNode({ name: 'linux-server' }),
        session_count: 2,
      },
    ];
    render(
      <OceanCanvas localNode={makeNode()} localSessions={[]} peers={peers} peerSessions={{}} />,
    );
    expect(screen.getByText('linux-server')).toBeInTheDocument();
  });

  it('renders peer sessions as octopuses', () => {
    const peers: PeerInfo[] = [
      {
        name: 'linux-server',
        address: '100.64.1.2:7433',
        status: 'online',
        node_info: makeNode({ name: 'linux-server' }),
        session_count: 1,
      },
    ];
    const peerSessions = {
      'linux-server': [makeSession({ name: 'peer-task', provider: 'codex' })],
    };
    render(
      <OceanCanvas
        localNode={makeNode()}
        localSessions={[]}
        peers={peers}
        peerSessions={peerSessions}
      />,
    );
    expect(screen.getByTestId('octopus-peer-task')).toBeInTheDocument();
  });

  it('shows ink on octopuses', () => {
    const sessions = [makeSession({ name: 'inked', ink: 'reviewer' })];
    render(
      <OceanCanvas localNode={makeNode()} localSessions={sessions} peers={[]} peerSessions={{}} />,
    );
    expect(screen.getByText('reviewer')).toBeInTheDocument();
  });

  it('shows empty ocean message when no sessions', () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    expect(screen.getByText(/no active sessions/i)).toBeInTheDocument();
  });

  it('colors offline peer islands differently', () => {
    const peers: PeerInfo[] = [
      {
        name: 'offline-node',
        address: '100.64.1.3:7433',
        status: 'offline',
        node_info: null,
        session_count: null,
      },
    ];
    render(
      <OceanCanvas localNode={makeNode()} localSessions={[]} peers={peers} peerSessions={{}} />,
    );
    const label = screen.getByText('offline-node');
    expect(label).toBeInTheDocument();
  });

  it('renders different statuses with correct octopus classes', () => {
    const sessions = [
      makeSession({ name: 'run', status: 'running' }),
      makeSession({ name: 'stl', status: 'stale', id: 's2' }),
      makeSession({ name: 'ded', status: 'dead', id: 's3' }),
      makeSession({ name: 'cmp', status: 'completed', id: 's4' }),
    ];
    render(
      <OceanCanvas localNode={makeNode()} localSessions={sessions} peers={[]} peerSessions={{}} />,
    );
    expect(screen.getByTestId('octopus-run').classList.contains('octopus-running')).toBe(true);
    expect(screen.getByTestId('octopus-stl').classList.contains('octopus-stale')).toBe(true);
    expect(screen.getByTestId('octopus-ded').classList.contains('octopus-dead')).toBe(true);
    expect(screen.getByTestId('octopus-cmp').classList.contains('octopus-completed')).toBe(true);
  });
});
