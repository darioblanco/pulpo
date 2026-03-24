import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { SessionDetailPage } from './session-detail';
import * as api from '@/api/client';
import type { Session, InterventionEvent } from '@/api/types';

vi.mock('@/api/client', () => ({
  getSession: vi.fn(),
  getInterventionEvents: vi.fn(),
  stopSession: vi.fn(),
  resumeSession: vi.fn(),
  downloadSessionOutput: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  resolveWsUrl: vi.fn().mockReturnValue('ws://localhost/test'),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.mock('@/components/session/terminal-view', () => ({
  TerminalView: ({ sessionId }: { sessionId: string }) => (
    <div data-testid="terminal-view">Terminal: {sessionId}</div>
  ),
}));

vi.mock('@/components/session/output-view', () => ({
  OutputView: ({ sessionId, sessionStatus }: { sessionId: string; sessionStatus: string }) => (
    <div data-testid="output-view">
      Output: {sessionId} ({sessionStatus})
    </div>
  ),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

class MockEventSource {
  url: string;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners: Record<string, ((e: { data: string }) => void)[]> = {};

  constructor(url: string) {
    this.url = url;
  }

  addEventListener(type: string, handler: (e: { data: string }) => void) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(handler);
  }

  close() {}
}

vi.stubGlobal('EventSource', MockEventSource);

const mockGetSession = vi.mocked(api.getSession);
const mockGetInterventions = vi.mocked(api.getInterventionEvents);
const mockStopSession = vi.mocked(api.stopSession);
const mockResumeSession = vi.mocked(api.resumeSession);

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-123',
    name: 'my-session',
    status: 'active',
    command: 'claude -p "fix bug"',
    description: null,
    workdir: '/home/user/project',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,
    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

function makeIntervention(overrides: Partial<InterventionEvent> = {}): InterventionEvent {
  return {
    id: 1,
    session_id: 'sess-123',
    reason: 'Memory threshold exceeded',
    created_at: '2025-01-01T01:00:00Z',
    ...overrides,
  };
}

const mockNavigate = vi.fn();
vi.mock('react-router', async () => {
  const actual = await vi.importActual('react-router');
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

beforeEach(() => {
  mockGetSession.mockReset();
  mockGetInterventions.mockReset();
  mockStopSession.mockReset();
  mockResumeSession.mockReset();
  mockNavigate.mockReset();
  mockGetInterventions.mockResolvedValue([]);
});

function renderDetail(sessionId = 'sess-123') {
  return render(
    <MemoryRouter initialEntries={[`/sessions/${sessionId}`]}>
      <ConnectionProvider>
        <SSEProvider>
          <TooltipProvider>
            <SidebarProvider>
              <Routes>
                <Route path="/sessions/:id" element={<SessionDetailPage />} />
              </Routes>
            </SidebarProvider>
          </TooltipProvider>
        </SSEProvider>
      </ConnectionProvider>
    </MemoryRouter>,
  );
}

describe('SessionDetailPage', () => {
  it('shows loading skeleton initially', () => {
    mockGetSession.mockResolvedValue(makeSession());
    renderDetail();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('renders session info after loading', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({ description: 'Fix the login bug', ink: 'claude-code' }),
    );
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toHaveTextContent('my-session');
    });
    expect(screen.getByTestId('session-status')).toHaveTextContent('active');
    expect(screen.getByTestId('session-command')).toHaveTextContent('claude -p "fix bug"');
    expect(screen.getByTestId('session-workdir')).toHaveTextContent('/home/user/project');
    expect(screen.getByTestId('session-ink')).toHaveTextContent('claude-code');
    expect(screen.getByTestId('session-description')).toHaveTextContent('Fix the login bug');
    expect(screen.getByTestId('session-id')).toHaveTextContent('sess-123');
  });

  it('shows worktree branch and path when set', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({
        worktree_branch: 'fix-auth',
        worktree_path: '/home/user/.pulpo/worktrees/fix-auth',
      }),
    );
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-worktree-branch')).toHaveTextContent('fix-auth');
      expect(screen.getByTestId('session-worktree-path')).toHaveTextContent(
        '/home/user/.pulpo/worktrees/fix-auth',
      );
    });
  });

  it('hides worktree info when not set', async () => {
    mockGetSession.mockResolvedValue(makeSession());
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('session-worktree-branch')).not.toBeInTheDocument();
    expect(screen.queryByTestId('session-worktree-path')).not.toBeInTheDocument();
  });

  it('shows terminal for active sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'active' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('terminal-section')).toBeInTheDocument();
    });
    expect(screen.getByTestId('terminal-view')).toBeInTheDocument();
    expect(screen.queryByTestId('output-section')).not.toBeInTheDocument();
  });

  it('shows terminal for idle sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'idle' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('terminal-section')).toBeInTheDocument();
    });
  });

  it('shows output view for ready sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'ready' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('output-section')).toBeInTheDocument();
    });
    expect(screen.getByTestId('output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('terminal-section')).not.toBeInTheDocument();
  });

  it('shows output view for stopped sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'stopped' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('output-section')).toBeInTheDocument();
    });
  });

  it('shows output view for lost sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'lost' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('output-section')).toBeInTheDocument();
    });
  });

  it('shows intervention history when present', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({
        status: 'stopped',
        intervention_reason: 'Memory threshold exceeded',
        intervention_at: '2025-01-01T01:00:00Z',
      }),
    );
    mockGetInterventions.mockResolvedValue([
      makeIntervention({ id: 1, reason: 'Memory breach #1' }),
      makeIntervention({ id: 2, reason: 'Memory breach #2' }),
    ]);
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('latest-intervention')).toBeInTheDocument();
    });
    expect(screen.getByTestId('intervention-list')).toBeInTheDocument();
    expect(screen.getByText('Memory breach #1')).toBeInTheDocument();
    expect(screen.getByText('Memory breach #2')).toBeInTheDocument();
  });

  it('shows no interventions message when empty', async () => {
    mockGetSession.mockResolvedValue(makeSession());
    mockGetInterventions.mockResolvedValue([]);
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('no-interventions')).toBeInTheDocument();
    });
    expect(screen.getByTestId('no-interventions')).toHaveTextContent('No interventions');
  });

  it('stop button calls stopSession', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'active' }));
    mockStopSession.mockResolvedValue(undefined);
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('btn-stop')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('btn-stop'));
    await waitFor(() => {
      expect(mockStopSession).toHaveBeenCalledWith('sess-123');
    });
  });

  it('resume button calls resumeSession', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'lost' }));
    mockResumeSession.mockResolvedValue({ id: 'sess-123', status: 'active' });
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('btn-resume')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('btn-resume'));
    await waitFor(() => {
      expect(mockResumeSession).toHaveBeenCalledWith('sess-123');
    });
  });

  it('purge button calls stopSession with purge and navigates', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'stopped' }));
    mockStopSession.mockResolvedValue(undefined);
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('btn-purge')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('btn-purge'));
    await waitFor(() => {
      expect(mockStopSession).toHaveBeenCalledWith('sess-123', true);
    });
    await waitFor(() => {
      expect(mockNavigate).toHaveBeenCalledWith('/sessions');
    });
  });

  it('back button navigates back', async () => {
    mockGetSession.mockResolvedValue(makeSession());
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('btn-back')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('btn-back'));
    expect(mockNavigate).toHaveBeenCalledWith(-1);
  });

  it('shows error when fetch fails', async () => {
    mockGetSession.mockRejectedValue(new Error('Network error'));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Failed to load session')).toBeInTheDocument();
    });
  });

  it('does not show stop button for ready sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'ready' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('btn-stop')).not.toBeInTheDocument();
  });

  it('does not show resume button for active sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'active' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('btn-resume')).not.toBeInTheDocument();
  });

  it('shows resume button for ready sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'ready' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('btn-resume')).toBeInTheDocument();
    });
  });

  it('shows purge button for lost sessions', async () => {
    mockGetSession.mockResolvedValue(makeSession({ status: 'lost' }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('btn-purge')).toBeInTheDocument();
    });
  });

  it('does not show ink or description when not set', async () => {
    mockGetSession.mockResolvedValue(makeSession({ ink: null, description: null }));
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('session-ink')).not.toBeInTheDocument();
    expect(screen.queryByTestId('session-description')).not.toBeInTheDocument();
  });

  it('shows latest intervention info on the session', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({
        intervention_reason: 'Idle timeout',
        intervention_at: '2025-01-01T02:00:00Z',
      }),
    );
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('latest-intervention')).toBeInTheDocument();
    });
    expect(screen.getByText('Idle timeout')).toBeInTheDocument();
  });

  it('shows git branch and commit when set', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({
        git_branch: 'feature/login',
        git_commit: 'abc1234',
      }),
    );
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-git-branch')).toHaveTextContent('feature/login');
      expect(screen.getByTestId('session-git-commit')).toHaveTextContent('abc1234');
    });
  });

  it('hides git info when not set', async () => {
    mockGetSession.mockResolvedValue(makeSession());
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('session-git-branch')).not.toBeInTheDocument();
    expect(screen.queryByTestId('session-git-commit')).not.toBeInTheDocument();
  });

  it('shows git diff stats when present', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({ git_insertions: 42, git_deletions: 7, git_files_changed: 3 }),
    );
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-git-diff')).toBeInTheDocument();
    });
    expect(screen.getByTestId('session-git-diff')).toHaveTextContent('+42');
    expect(screen.getByTestId('session-git-diff')).toHaveTextContent('-7');
    expect(screen.getByTestId('session-git-diff')).toHaveTextContent('3 files');
  });

  it('hides git diff when zero', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({ git_insertions: 0, git_deletions: 0, git_files_changed: 0 }),
    );
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('session-git-diff')).not.toBeInTheDocument();
  });

  it('shows git ahead when > 0', async () => {
    mockGetSession.mockResolvedValue(makeSession({ git_ahead: 5 }));
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-git-ahead')).toBeInTheDocument();
    });
    expect(screen.getByTestId('session-git-ahead')).toHaveTextContent('5');
  });

  it('hides git ahead when 0', async () => {
    mockGetSession.mockResolvedValue(makeSession({ git_ahead: 0 }));
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-name')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('session-git-ahead')).not.toBeInTheDocument();
  });

  it('shows error status from metadata', async () => {
    mockGetSession.mockResolvedValue(makeSession({ metadata: { error_status: 'Compile error' } }));
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-error')).toBeInTheDocument();
    });
    expect(screen.getByTestId('session-error')).toHaveTextContent('Compile error');
  });

  it('shows token usage from metadata', async () => {
    mockGetSession.mockResolvedValue(
      makeSession({
        metadata: { total_input_tokens: '12345', total_output_tokens: '6789' },
      }),
    );
    renderDetail();
    await waitFor(() => {
      expect(screen.getByTestId('session-tokens')).toBeInTheDocument();
    });
    expect(screen.getByTestId('session-tokens')).toHaveTextContent('12,345');
    expect(screen.getByTestId('session-tokens')).toHaveTextContent('6,789');
  });
});
