import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { toast } from 'sonner';
import { SessionCard } from './session-card';
import * as api from '@/api/client';
import type { Session } from '@/api/types';

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
    status: 'active',
    command: 'Fix the bug',
    description: null,
    workdir: '/home/user/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,

    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

function renderCard(session: Session, onRefresh = vi.fn()) {
  return render(
    <MemoryRouter>
      <SessionCard session={session} onRefresh={onRefresh} />
    </MemoryRouter>,
  );
}

function clickExpand() {
  fireEvent.click(screen.getByTestId('btn-expand'));
}

describe('SessionCard', () => {
  it('renders session name, command, status', () => {
    renderCard(makeSession());
    expect(screen.getByText('my-api')).toBeInTheDocument();
    expect(screen.getByText('active')).toBeInTheDocument();
    expect(screen.getAllByText('Fix the bug').length).toBeGreaterThan(0);
  });

  it('shows workdir basename in header', () => {
    renderCard(makeSession());
    expect(screen.getByTestId('session-workdir')).toHaveTextContent('repo');
    expect(screen.getByTestId('session-workdir-short')).toHaveTextContent('repo');
  });

  it('shows ink when set', () => {
    renderCard(makeSession({ ink: 'reviewer' }));
    expect(screen.getByTestId('session-ink')).toHaveTextContent('reviewer');
  });

  it('hides ink when null', () => {
    renderCard(makeSession());
    expect(screen.queryByTestId('session-ink')).not.toBeInTheDocument();
  });

  // Traffic light buttons

  it('enables kill dot for active sessions', () => {
    renderCard(makeSession());
    expect(screen.getByTestId('btn-kill')).not.toBeDisabled();
  });

  it('enables kill dot for lost sessions', () => {
    renderCard(makeSession({ status: 'lost' }));
    expect(screen.getByTestId('btn-kill')).not.toBeDisabled();
  });

  it('disables kill dot for ready sessions', () => {
    renderCard(makeSession({ status: 'ready' }));
    expect(screen.getByTestId('btn-kill')).toBeDisabled();
  });

  it('enables resume dot only for lost sessions', () => {
    renderCard(makeSession({ status: 'lost' }));
    expect(screen.getByTestId('btn-resume')).not.toBeDisabled();
  });

  it('disables resume dot for active sessions', () => {
    renderCard(makeSession());
    expect(screen.getByTestId('btn-resume')).toBeDisabled();
  });

  // Expand/collapse

  it('toggles expanded state on green dot click', () => {
    renderCard(makeSession());
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
    clickExpand();
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
  });

  it('collapses on second green dot click', () => {
    renderCard(makeSession());
    clickExpand();
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
    clickExpand();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('expands on title bar click', () => {
    renderCard(makeSession());
    fireEvent.click(screen.getByTestId('session-name-link'));
    // Clicking the name now navigates instead of expanding
  });

  it('expands via keyboard Enter on header', () => {
    renderCard(makeSession());
    const infoArea = screen.getByTestId('session-name-link').closest('[role="button"]')!;
    fireEvent.keyDown(infoArea, { key: 'Enter' });
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
  });

  // View switching

  it('shows TerminalView for active session', () => {
    renderCard(makeSession());
    clickExpand();
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-output-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for ready session', () => {
    renderCard(makeSession({ status: 'ready' }));
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for killed session', () => {
    renderCard(makeSession({ status: 'killed' }));
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for lost session', () => {
    renderCard(makeSession({ status: 'lost' }));
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  // Kill action

  it('shows confirmation dialog on red dot click', async () => {
    renderCard(makeSession());
    fireEvent.click(screen.getByTestId('btn-kill'));
    await waitFor(() => {
      expect(screen.getByText(/Kill session "my-api"/)).toBeInTheDocument();
      expect(screen.getByText('Cancel')).toBeInTheDocument();
    });
  });

  it('calls killSession after confirming dialog', async () => {
    mockKillSession.mockResolvedValue(undefined);
    const onRefresh = vi.fn();
    renderCard(makeSession(), onRefresh);
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
    renderCard(makeSession(), onRefresh);
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
    mockResumeSession.mockResolvedValue({ id: 'sess-1', status: 'active' });
    const onRefresh = vi.fn();
    renderCard(makeSession({ status: 'lost' }), onRefresh);
    fireEvent.click(screen.getByTestId('btn-resume'));
    await waitFor(() => {
      expect(mockResumeSession).toHaveBeenCalledWith('sess-1');
      expect(onRefresh).toHaveBeenCalled();
    });
  });

  it('shows toast on resume error', async () => {
    mockResumeSession.mockRejectedValue(new Error('Resume failed'));
    const onRefresh = vi.fn();
    renderCard(makeSession({ status: 'lost' }), onRefresh);
    fireEvent.click(screen.getByTestId('btn-resume'));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Resume failed');
    });
    expect(onRefresh).not.toHaveBeenCalled();
  });

  // Fullscreen

  it('shows fullscreen button when expanded for active session', () => {
    renderCard(makeSession());
    clickExpand();
    expect(screen.getByTestId('btn-fullscreen')).toBeInTheDocument();
  });

  it('does not show fullscreen button for non-active sessions', () => {
    renderCard(makeSession({ status: 'ready' }));
    clickExpand();
    expect(screen.queryByTestId('btn-fullscreen')).not.toBeInTheDocument();
  });

  it('opens fullscreen terminal overlay on click', () => {
    renderCard(makeSession());
    clickExpand();
    fireEvent.click(screen.getByTestId('btn-fullscreen'));
    expect(screen.getByTestId('fullscreen-terminal')).toBeInTheDocument();
    expect(screen.getByText('Close')).toBeInTheDocument();
  });

  it('closes fullscreen terminal overlay on close click', () => {
    renderCard(makeSession());
    clickExpand();
    fireEvent.click(screen.getByTestId('btn-fullscreen'));
    expect(screen.getByTestId('fullscreen-terminal')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('btn-fullscreen-close'));
    expect(screen.queryByTestId('fullscreen-terminal')).not.toBeInTheDocument();
  });

  // Intervention

  // Worktree badge

  it('shows worktree badge when worktree_path is set', () => {
    renderCard(makeSession({ worktree_path: '/repo/.pulpo/worktrees/my-task' }));
    const badge = screen.getByTestId('worktree-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('my-task');
  });

  it('hides worktree badge when worktree_path is null', () => {
    renderCard(makeSession({ worktree_path: null }));
    expect(screen.queryByTestId('worktree-badge')).not.toBeInTheDocument();
  });

  it('hides worktree badge when worktree_path is undefined', () => {
    renderCard(makeSession());
    expect(screen.queryByTestId('worktree-badge')).not.toBeInTheDocument();
  });

  // Intervention

  it('shows intervention badge for killed sessions', () => {
    renderCard(
      makeSession({
        status: 'killed',
        intervention_reason: 'Memory exceeded',
        intervention_at: '2026-01-01T12:00:00Z',
      }),
    );
    expect(screen.getByTestId('intervention-badge')).toBeInTheDocument();
    expect(screen.getByText('intervened')).toBeInTheDocument();
  });

  it('does not show intervention badge without reason', () => {
    renderCard(makeSession({ status: 'killed' }));
    expect(screen.queryByTestId('intervention-badge')).not.toBeInTheDocument();
  });

  it('shows intervention badge for killed sessions only', () => {
    renderCard(makeSession({ status: 'ready', intervention_reason: 'test' }));
    expect(screen.queryByTestId('intervention-badge')).not.toBeInTheDocument();
  });

  it('shows intervention details when expanded', () => {
    renderCard(
      makeSession({
        status: 'killed',
        intervention_reason: 'Memory exceeded',
        intervention_at: '2026-01-01T12:00:00Z',
      }),
    );
    clickExpand();
    expect(screen.getByText(/Memory exceeded/)).toBeInTheDocument();
    expect(screen.getByText('Show history')).toBeInTheDocument();
  });

  it('loads intervention history on toggle', async () => {
    mockGetInterventionEvents.mockResolvedValue([
      { id: 1, session_id: 'sess-1', reason: 'OOM kill', created_at: '2026-01-01T12:00:00Z' },
    ]);
    renderCard(
      makeSession({
        status: 'killed',
        intervention_reason: 'Memory exceeded',
        intervention_at: '2026-01-01T12:00:00Z',
      }),
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
