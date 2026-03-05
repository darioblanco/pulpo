import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { toast } from 'sonner';
import { SessionCard } from './session-card';
import * as api from '@/api/client';
import type { Session, GuardConfig } from '@/api/types';

vi.mock('sonner', () => ({
  toast: { error: vi.fn() },
}));

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

// Mock OutputView and TerminalView to avoid complex dependencies
vi.mock('@/components/session/output-view', () => ({
  OutputView: ({ sessionId }: { sessionId: string }) => (
    <div data-testid="mock-output-view">OutputView:{sessionId}</div>
  ),
}));

vi.mock('@/components/session/terminal-view', () => ({
  TerminalView: ({ sessionId }: { sessionId: string }) => (
    <div data-testid="mock-terminal-view">Terminal:{sessionId}</div>
  ),
}));

const mockKillSession = vi.mocked(api.killSession);
const mockResumeSession = vi.mocked(api.resumeSession);
const mockGetInterventionEvents = vi.mocked(api.getInterventionEvents);

beforeEach(() => {
  mockKillSession.mockReset();
  mockResumeSession.mockReset();
  mockGetInterventionEvents.mockReset();
});

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

function makeGuard(preset: string): GuardConfig {
  return { preset };
}

function clickHeader() {
  const header = screen.getByTestId('session-header');
  fireEvent.click(header);
}

describe('SessionCard', () => {
  it('renders session name, provider, mode, status, and prompt', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.getByText('my-api')).toBeInTheDocument();
    expect(screen.getByText('claude')).toBeInTheDocument();
    expect(screen.getByText('interactive')).toBeInTheDocument();
    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('Fix the bug')).toBeInTheDocument();
  });

  it('shows guard badge', () => {
    render(
      <SessionCard
        session={makeSession({ guard_config: makeGuard('strict') })}
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.getByTestId('guard-badge')).toBeInTheDocument();
    expect(screen.getByText('strict')).toBeInTheDocument();
  });

  it('shows no guard badge when null', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.queryByTestId('guard-badge')).not.toBeInTheDocument();
  });

  it('toggles expanded state on header click', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.queryByText('Kill Session')).not.toBeInTheDocument();
    clickHeader();
    expect(screen.getByText('Kill Session')).toBeInTheDocument();
  });

  it('collapses on second click', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    clickHeader();
    expect(screen.getByText('Kill Session')).toBeInTheDocument();
    clickHeader();
    expect(screen.queryByText('Kill Session')).not.toBeInTheDocument();
  });

  it('shows TerminalView for running session', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    clickHeader();
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-output-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for completed session', () => {
    render(<SessionCard session={makeSession({ status: 'completed' })} onRefresh={vi.fn()} />);
    clickHeader();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for dead session', () => {
    render(<SessionCard session={makeSession({ status: 'dead' })} onRefresh={vi.fn()} />);
    clickHeader();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for stale session', () => {
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={vi.fn()} />);
    clickHeader();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('calls killSession on Kill button click', async () => {
    mockKillSession.mockResolvedValue(undefined);
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession()} onRefresh={onRefresh} />);
    clickHeader();
    fireEvent.click(screen.getByText('Kill Session'));
    await waitFor(() => {
      expect(mockKillSession).toHaveBeenCalledWith('sess-1');
      expect(onRefresh).toHaveBeenCalled();
    });
  });

  it('shows Resume and Kill for stale sessions', () => {
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={vi.fn()} />);
    clickHeader();
    expect(screen.getByText('Resume')).toBeInTheDocument();
    expect(screen.getByText('Kill Session')).toBeInTheDocument();
  });

  it('calls resumeSession on Resume click', async () => {
    mockResumeSession.mockResolvedValue({ id: 'sess-1', status: 'running' });
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={onRefresh} />);
    clickHeader();
    fireEvent.click(screen.getByText('Resume'));
    await waitFor(() => {
      expect(mockResumeSession).toHaveBeenCalledWith('sess-1');
      expect(onRefresh).toHaveBeenCalled();
    });
  });

  it('shows intervention badge for dead sessions', () => {
    render(
      <SessionCard
        session={makeSession({
          status: 'dead',
          intervention_reason: 'Memory exceeded',
          intervention_at: '2026-01-01T12:00:00Z',
        })}
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.getByTestId('intervention-badge')).toBeInTheDocument();
    expect(screen.getByText('intervened')).toBeInTheDocument();
  });

  it('does not show intervention badge without reason', () => {
    render(<SessionCard session={makeSession({ status: 'dead' })} onRefresh={vi.fn()} />);
    expect(screen.queryByTestId('intervention-badge')).not.toBeInTheDocument();
  });

  it('shows intervention details when expanded', () => {
    render(
      <SessionCard
        session={makeSession({
          status: 'dead',
          intervention_reason: 'Memory exceeded',
          intervention_at: '2026-01-01T12:00:00Z',
        })}
        onRefresh={vi.fn()}
      />,
    );
    clickHeader();
    expect(screen.getByText(/Memory exceeded/)).toBeInTheDocument();
    expect(screen.getByText('Show history')).toBeInTheDocument();
  });

  it('loads intervention history on toggle', async () => {
    mockGetInterventionEvents.mockResolvedValue([
      { id: 1, session_id: 'sess-1', reason: 'OOM kill', created_at: '2026-01-01T12:00:00Z' },
    ]);
    render(
      <SessionCard
        session={makeSession({
          status: 'dead',
          intervention_reason: 'Memory exceeded',
          intervention_at: '2026-01-01T12:00:00Z',
        })}
        onRefresh={vi.fn()}
      />,
    );
    clickHeader();
    fireEvent.click(screen.getByTestId('interventions-toggle'));
    await waitFor(() => {
      expect(mockGetInterventionEvents).toHaveBeenCalledWith('sess-1');
      expect(screen.getByTestId('intervention-history')).toBeInTheDocument();
      expect(screen.getByText('OOM kill')).toBeInTheDocument();
      expect(screen.getByText('Hide history')).toBeInTheDocument();
    });
  });

  it('expands via keyboard Enter', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    const header = screen.getByTestId('session-header');
    fireEvent.keyDown(header, { key: 'Enter' });
    expect(screen.getByText('Kill Session')).toBeInTheDocument();
  });

  it('shows toast on kill error', async () => {
    mockKillSession.mockRejectedValue(new Error('Kill failed'));
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession()} onRefresh={onRefresh} />);
    clickHeader();
    fireEvent.click(screen.getByText('Kill Session'));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Kill failed');
    });
    expect(onRefresh).not.toHaveBeenCalled();
  });

  it('shows toast on resume error', async () => {
    mockResumeSession.mockRejectedValue(new Error('Resume failed'));
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={onRefresh} />);
    clickHeader();
    fireEvent.click(screen.getByText('Resume'));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Resume failed');
    });
    expect(onRefresh).not.toHaveBeenCalled();
  });
});
