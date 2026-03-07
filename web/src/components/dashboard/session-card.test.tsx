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

function clickExpand() {
  fireEvent.click(screen.getByTestId('btn-expand'));
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

  it('shows workdir basename in header', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.getByTestId('session-workdir')).toHaveTextContent('repo');
  });

  it('shows persona and model when set', () => {
    render(
      <SessionCard
        session={makeSession({ persona: 'reviewer', model: 'opus-4' })}
        onRefresh={vi.fn()}
      />,
    );
    expect(screen.getByTestId('session-persona')).toHaveTextContent('reviewer');
    expect(screen.getByTestId('session-model')).toHaveTextContent('opus-4');
  });

  it('hides persona and model when null', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.queryByTestId('session-persona')).not.toBeInTheDocument();
    expect(screen.queryByTestId('session-model')).not.toBeInTheDocument();
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

  // Traffic light buttons

  it('enables kill dot for running sessions', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.getByTestId('btn-kill')).not.toBeDisabled();
  });

  it('enables kill dot for stale sessions', () => {
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={vi.fn()} />);
    expect(screen.getByTestId('btn-kill')).not.toBeDisabled();
  });

  it('disables kill dot for completed sessions', () => {
    render(<SessionCard session={makeSession({ status: 'completed' })} onRefresh={vi.fn()} />);
    expect(screen.getByTestId('btn-kill')).toBeDisabled();
  });

  it('enables resume dot only for stale sessions', () => {
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={vi.fn()} />);
    expect(screen.getByTestId('btn-resume')).not.toBeDisabled();
  });

  it('disables resume dot for running sessions', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.getByTestId('btn-resume')).toBeDisabled();
  });

  // Expand/collapse

  it('toggles expanded state on green dot click', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
    clickExpand();
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
  });

  it('collapses on second green dot click', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    clickExpand();
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
    clickExpand();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('expands on title bar click', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByText('my-api'));
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
  });

  it('expands via keyboard Enter on header', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    const infoArea = screen.getByText('my-api').closest('[role="button"]')!;
    fireEvent.keyDown(infoArea, { key: 'Enter' });
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
  });

  // View switching

  it('shows TerminalView for running session', () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    clickExpand();
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-output-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for completed session', () => {
    render(<SessionCard session={makeSession({ status: 'completed' })} onRefresh={vi.fn()} />);
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for dead session', () => {
    render(<SessionCard session={makeSession({ status: 'dead' })} onRefresh={vi.fn()} />);
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for stale session', () => {
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={vi.fn()} />);
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  // Kill action

  it('shows confirmation dialog on red dot click', async () => {
    render(<SessionCard session={makeSession()} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByTestId('btn-kill'));
    await waitFor(() => {
      expect(screen.getByText(/Kill session "my-api"/)).toBeInTheDocument();
      expect(screen.getByText('Cancel')).toBeInTheDocument();
    });
  });

  it('calls killSession after confirming dialog', async () => {
    mockKillSession.mockResolvedValue(undefined);
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession()} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('btn-kill'));
    await waitFor(() => {
      expect(screen.getByTestId('btn-kill-confirm')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId('btn-kill-confirm'));
    await waitFor(() => {
      expect(mockKillSession).toHaveBeenCalledWith('sess-1');
      expect(onRefresh).toHaveBeenCalled();
    });
  });

  it('shows toast on kill error', async () => {
    mockKillSession.mockRejectedValue(new Error('Kill failed'));
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession()} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('btn-kill'));
    await waitFor(() => {
      expect(screen.getByTestId('btn-kill-confirm')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId('btn-kill-confirm'));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Kill failed');
    });
    expect(onRefresh).not.toHaveBeenCalled();
  });

  // Resume action

  it('calls resumeSession on yellow dot click', async () => {
    mockResumeSession.mockResolvedValue({ id: 'sess-1', status: 'running' });
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('btn-resume'));
    await waitFor(() => {
      expect(mockResumeSession).toHaveBeenCalledWith('sess-1');
      expect(onRefresh).toHaveBeenCalled();
    });
  });

  it('shows toast on resume error', async () => {
    mockResumeSession.mockRejectedValue(new Error('Resume failed'));
    const onRefresh = vi.fn();
    render(<SessionCard session={makeSession({ status: 'stale' })} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('btn-resume'));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Resume failed');
    });
    expect(onRefresh).not.toHaveBeenCalled();
  });

  // Intervention

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
    clickExpand();
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
    clickExpand();
    fireEvent.click(screen.getByTestId('interventions-toggle'));
    await waitFor(() => {
      expect(mockGetInterventionEvents).toHaveBeenCalledWith('sess-1');
      expect(screen.getByTestId('intervention-history')).toBeInTheDocument();
      expect(screen.getByText('OOM kill')).toBeInTheDocument();
      expect(screen.getByText('Hide history')).toBeInTheDocument();
    });
  });
});
