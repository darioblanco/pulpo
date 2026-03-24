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

const mockStopSession = vi.mocked(api.stopSession);
const mockResumeSession = vi.mocked(api.resumeSession);
const mockGetInterventionEvents = vi.mocked(api.getInterventionEvents);
const mockSendInput = vi.mocked(api.sendInput);

beforeEach(() => {
  mockStopSession.mockReset();
  mockResumeSession.mockReset();
  mockGetInterventionEvents.mockReset();
  mockSendInput.mockReset();
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

  it('enables stop dot for active sessions', () => {
    renderCard(makeSession());
    expect(screen.getByTestId('btn-stop')).not.toBeDisabled();
  });

  it('enables stop dot for idle sessions', () => {
    renderCard(makeSession({ status: 'idle' }));
    expect(screen.getByTestId('btn-stop')).not.toBeDisabled();
  });

  it('enables stop dot for lost sessions', () => {
    renderCard(makeSession({ status: 'lost' }));
    expect(screen.getByTestId('btn-stop')).not.toBeDisabled();
  });

  it('disables stop dot for ready sessions', () => {
    renderCard(makeSession({ status: 'ready' }));
    expect(screen.getByTestId('btn-stop')).toBeDisabled();
  });

  it('enables resume dot for lost sessions', () => {
    renderCard(makeSession({ status: 'lost' }));
    expect(screen.getByTestId('btn-resume')).not.toBeDisabled();
  });

  it('enables resume dot for ready sessions', () => {
    renderCard(makeSession({ status: 'ready' }));
    expect(screen.getByTestId('btn-resume')).not.toBeDisabled();
  });

  it('disables resume dot for active sessions', () => {
    renderCard(makeSession());
    expect(screen.getByTestId('btn-resume')).toBeDisabled();
  });

  // Expand/collapse

  it('toggles expanded state on green dot click', () => {
    renderCard(makeSession());
    expect(screen.queryByTestId('mock-output-view')).not.toBeInTheDocument();
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
  });

  it('collapses on second green dot click', () => {
    renderCard(makeSession());
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    clickExpand();
    expect(screen.queryByTestId('mock-output-view')).not.toBeInTheDocument();
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
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
  });

  // View switching

  it('shows OutputView by default for active session', () => {
    renderCard(makeSession());
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows view toggle button for active sessions', () => {
    renderCard(makeSession());
    clickExpand();
    expect(screen.getByTestId('btn-view-toggle')).toBeInTheDocument();
    expect(screen.getByTestId('btn-view-toggle')).toHaveTextContent('Terminal');
  });

  it('toggles to TerminalView when toggle button clicked', () => {
    renderCard(makeSession());
    clickExpand();
    fireEvent.click(screen.getByTestId('btn-view-toggle'));
    expect(screen.getByTestId('mock-terminal-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-output-view')).not.toBeInTheDocument();
    expect(screen.getByTestId('btn-view-toggle')).toHaveTextContent('Output');
  });

  it('shows OutputView for idle session when expanded', () => {
    renderCard(makeSession({ status: 'idle' }));
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.getByTestId('btn-view-toggle')).toBeInTheDocument();
  });

  it('shows OutputView for ready session', () => {
    renderCard(makeSession({ status: 'ready' }));
    clickExpand();
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('mock-terminal-view')).not.toBeInTheDocument();
  });

  it('shows OutputView for stopped session', () => {
    renderCard(makeSession({ status: 'stopped' }));
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

  // Stop action

  it('shows confirmation dialog on red dot click', async () => {
    renderCard(makeSession());
    fireEvent.click(screen.getByTestId('btn-stop'));
    await waitFor(() => {
      expect(screen.getByText(/Stop session "my-api"/)).toBeInTheDocument();
      expect(screen.getByText('Cancel')).toBeInTheDocument();
    });
  });

  it('calls stopSession after confirming dialog', async () => {
    mockStopSession.mockResolvedValue(undefined);
    const onRefresh = vi.fn();
    renderCard(makeSession(), onRefresh);
    fireEvent.click(screen.getByTestId('btn-stop'));
    await waitFor(() => {
      expect(screen.getByTestId('btn-stop-confirm')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId('btn-stop-confirm'));
    await waitFor(() => {
      expect(mockStopSession).toHaveBeenCalledWith('sess-1');
      expect(onRefresh).toHaveBeenCalled();
    });
  });

  it('shows toast on stop error', async () => {
    mockStopSession.mockRejectedValue(new Error('Stop failed'));
    const onRefresh = vi.fn();
    renderCard(makeSession(), onRefresh);
    fireEvent.click(screen.getByTestId('btn-stop'));
    await waitFor(() => {
      expect(screen.getByTestId('btn-stop-confirm')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId('btn-stop-confirm'));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Stop failed');
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

  it('shows fullscreen button when expanded in terminal mode', () => {
    renderCard(makeSession());
    clickExpand();
    // Default is output mode — no fullscreen button
    expect(screen.queryByTestId('btn-fullscreen')).not.toBeInTheDocument();
    // Switch to terminal
    fireEvent.click(screen.getByTestId('btn-view-toggle'));
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
    fireEvent.click(screen.getByTestId('btn-view-toggle'));
    fireEvent.click(screen.getByTestId('btn-fullscreen'));
    expect(screen.getByTestId('fullscreen-terminal')).toBeInTheDocument();
    expect(screen.getByText('Close')).toBeInTheDocument();
  });

  it('closes fullscreen terminal overlay on close click', () => {
    renderCard(makeSession());
    clickExpand();
    fireEvent.click(screen.getByTestId('btn-view-toggle'));
    fireEvent.click(screen.getByTestId('btn-fullscreen'));
    expect(screen.getByTestId('fullscreen-terminal')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('btn-fullscreen-close'));
    expect(screen.queryByTestId('fullscreen-terminal')).not.toBeInTheDocument();
  });

  // Intervention

  // Worktree badge

  it('shows worktree badge with branch name when set', () => {
    renderCard(
      makeSession({
        worktree_path: '/repo/.pulpo/worktrees/my-task',
        worktree_branch: 'my-task',
      }),
    );
    const badge = screen.getByTestId('worktree-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('my-task');
  });

  it('shows worktree badge falling back to path when branch is not set', () => {
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

  it('shows worktree cleanup note for stopped session with worktree', () => {
    renderCard(makeSession({ status: 'stopped', worktree_path: '/repo/.pulpo/worktrees/my-task' }));
    clickExpand();
    expect(screen.getByTestId('worktree-cleaned')).toHaveTextContent('Worktree cleaned up');
  });

  it('shows worktree cleanup note for lost session with worktree', () => {
    renderCard(makeSession({ status: 'lost', worktree_path: '/repo/.pulpo/worktrees/my-task' }));
    clickExpand();
    expect(screen.getByTestId('worktree-cleaned')).toHaveTextContent('Worktree cleaned up');
  });

  it('does not show worktree cleanup note for active session', () => {
    renderCard(makeSession({ worktree_path: '/repo/.pulpo/worktrees/my-task' }));
    clickExpand();
    expect(screen.queryByTestId('worktree-cleaned')).not.toBeInTheDocument();
  });

  // Intervention

  it('shows intervention badge for stopped sessions', () => {
    renderCard(
      makeSession({
        status: 'stopped',
        intervention_reason: 'Memory exceeded',
        intervention_at: '2026-01-01T12:00:00Z',
      }),
    );
    expect(screen.getByTestId('intervention-badge')).toBeInTheDocument();
    expect(screen.getByText('intervened')).toBeInTheDocument();
  });

  it('does not show intervention badge without reason', () => {
    renderCard(makeSession({ status: 'stopped' }));
    expect(screen.queryByTestId('intervention-badge')).not.toBeInTheDocument();
  });

  it('shows intervention badge for stopped sessions only', () => {
    renderCard(makeSession({ status: 'ready', intervention_reason: 'test' }));
    expect(screen.queryByTestId('intervention-badge')).not.toBeInTheDocument();
  });

  it('shows intervention details when expanded', () => {
    renderCard(
      makeSession({
        status: 'stopped',
        intervention_reason: 'Memory exceeded',
        intervention_at: '2026-01-01T12:00:00Z',
      }),
    );
    clickExpand();
    expect(screen.getByText(/Memory exceeded/)).toBeInTheDocument();
    expect(screen.getByText('Show history')).toBeInTheDocument();
  });

  // PR badge

  it('shows PR badge when metadata has pr_url', () => {
    renderCard(makeSession({ metadata: { pr_url: 'https://github.com/a/b/pull/1' } }));
    const badge = screen.getByTestId('pr-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveAttribute('href', 'https://github.com/a/b/pull/1');
    expect(badge).toHaveAttribute('target', '_blank');
  });

  it('does not show PR badge when metadata is null', () => {
    renderCard(makeSession({ metadata: null }));
    expect(screen.queryByTestId('pr-badge')).not.toBeInTheDocument();
  });

  it('does not show PR badge when metadata has no pr_url', () => {
    renderCard(makeSession({ metadata: { other: 'value' } }));
    expect(screen.queryByTestId('pr-badge')).not.toBeInTheDocument();
  });

  // Branch badge

  it('shows branch badge when metadata has branch', () => {
    renderCard(makeSession({ metadata: { branch: 'feature/my-branch' } }));
    const badge = screen.getByTestId('branch-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('feature/my-branch');
  });

  it('does not show branch badge when no branch in metadata', () => {
    renderCard(makeSession({ metadata: { pr_url: 'https://github.com/a/b/pull/1' } }));
    expect(screen.queryByTestId('branch-badge')).not.toBeInTheDocument();
  });

  it('shows both PR badge and branch badge together', () => {
    renderCard(
      makeSession({
        metadata: { pr_url: 'https://github.com/a/b/pull/1', branch: 'my-branch' },
      }),
    );
    expect(screen.getByTestId('pr-badge')).toBeInTheDocument();
    expect(screen.getByTestId('branch-badge')).toBeInTheDocument();
  });

  // Auth plan badge

  it('shows auth plan badge when metadata has auth_plan', () => {
    renderCard(makeSession({ metadata: { auth_plan: 'max' } }));
    const badge = screen.getByTestId('auth-plan-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('max');
  });

  it('shows auth email as tooltip on plan badge', () => {
    renderCard(makeSession({ metadata: { auth_plan: 'pro', auth_email: 'user@example.com' } }));
    const badge = screen.getByTestId('auth-plan-badge');
    expect(badge).toHaveAttribute('title', 'user@example.com');
  });

  it('does not show auth plan badge when not in metadata', () => {
    renderCard(makeSession({ metadata: { pr_url: 'https://github.com/a/b/pull/1' } }));
    expect(screen.queryByTestId('auth-plan-badge')).not.toBeInTheDocument();
  });

  it('does not show auth plan badge when metadata is null', () => {
    renderCard(makeSession({ metadata: null }));
    expect(screen.queryByTestId('auth-plan-badge')).not.toBeInTheDocument();
  });

  // Rate limit badge

  it('shows rate limit badge when metadata has rate_limit', () => {
    renderCard(makeSession({ metadata: { rate_limit: 'Rate limited' } }));
    const badge = screen.getByTestId('rate-limit-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('Rate limited');
  });

  it('highlights rate limit badge when recent (within 5 minutes)', () => {
    const recentTime = new Date().toISOString();
    renderCard(
      makeSession({
        metadata: { rate_limit: 'Rate limited', rate_limit_at: recentTime },
      }),
    );
    const badge = screen.getByTestId('rate-limit-badge');
    expect(badge).toBeInTheDocument();
    // Recent: bright amber styling
    expect(badge.className).toContain('text-[#fbbf24]');
  });

  it('shows dimmer rate limit badge when older than 5 minutes', () => {
    const oldTime = new Date(Date.now() - 600_000).toISOString();
    renderCard(
      makeSession({
        metadata: { rate_limit: 'Rate limited', rate_limit_at: oldTime },
      }),
    );
    const badge = screen.getByTestId('rate-limit-badge');
    expect(badge).toBeInTheDocument();
    // Old: dimmer styling
    expect(badge.className).toContain('text-[#a89a6a]');
  });

  it('does not show rate limit badge when not in metadata', () => {
    renderCard(makeSession({ metadata: { branch: 'main' } }));
    expect(screen.queryByTestId('rate-limit-badge')).not.toBeInTheDocument();
  });

  it('does not show rate limit badge when metadata is null', () => {
    renderCard(makeSession({ metadata: null }));
    expect(screen.queryByTestId('rate-limit-badge')).not.toBeInTheDocument();
  });

  it('truncates long commands', () => {
    renderCard(makeSession({ command: 'a'.repeat(60) }));
    // The command text should be truncated with ...
    const commandElements = screen.getAllByText(/a+\.\.\./);
    expect(commandElements.length).toBeGreaterThan(0);
  });

  it('shows description in subtitle when available', () => {
    renderCard(makeSession({ description: 'Fix the login page' }));
    expect(screen.getByText('Fix the login page')).toBeInTheDocument();
  });

  it('shows command in subtitle when no description', () => {
    renderCard(makeSession({ description: null, command: 'Fix the bug' }));
    // Both the header command and subtitle show the command
    const elements = screen.getAllByText('Fix the bug');
    expect(elements.length).toBeGreaterThanOrEqual(2);
  });

  it('expands via Space key on header', () => {
    renderCard(makeSession());
    const infoArea = screen.getByTestId('session-name-link').closest('[role="button"]')!;
    fireEvent.keyDown(infoArea, { key: ' ' });
    expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
  });

  it('expands on subtitle click', () => {
    renderCard(makeSession());
    // Subtitle area is the second role="button"
    const subtitleArea = screen
      .getAllByRole('button')
      .find((el) => el.querySelector('.truncate.font-mono'));
    if (subtitleArea) {
      fireEvent.click(subtitleArea);
      expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    }
  });

  it('expands on subtitle keyboard Enter', () => {
    renderCard(makeSession());
    const subtitleAreas = screen.getAllByRole('button');
    // The subtitle area has the description/command text
    const subtitleArea = subtitleAreas.find((el) => el.querySelector('.truncate.font-mono'));
    if (subtitleArea) {
      fireEvent.keyDown(subtitleArea, { key: 'Enter' });
      expect(screen.getByTestId('mock-output-view')).toBeInTheDocument();
    }
  });

  it('shows rate limit title with timestamp', () => {
    const timestamp = '2026-03-20T10:00:00Z';
    renderCard(
      makeSession({
        metadata: { rate_limit: 'Rate limited', rate_limit_at: timestamp },
      }),
    );
    const badge = screen.getByTestId('rate-limit-badge');
    expect(badge.getAttribute('title')).toContain('Rate limited at');
  });

  it('shows rate limit title without timestamp', () => {
    renderCard(makeSession({ metadata: { rate_limit: 'Rate limited' } }));
    const badge = screen.getByTestId('rate-limit-badge');
    expect(badge.getAttribute('title')).toBe('Rate limited');
  });

  it('hides intervention history on second toggle', async () => {
    mockGetInterventionEvents.mockResolvedValue([
      { id: 1, session_id: 'sess-1', reason: 'OOM kill', created_at: '2026-01-01T12:00:00Z' },
    ]);
    renderCard(
      makeSession({
        status: 'stopped',
        intervention_reason: 'Memory exceeded',
        intervention_at: '2026-01-01T12:00:00Z',
      }),
    );
    clickExpand();

    // Open history
    fireEvent.click(screen.getByTestId('interventions-toggle'));
    await waitFor(() => {
      expect(screen.getByTestId('intervention-history')).toBeInTheDocument();
    });

    // Close history
    fireEvent.click(screen.getByTestId('interventions-toggle'));
    expect(screen.queryByTestId('intervention-history')).not.toBeInTheDocument();
    expect(screen.getByText('Show history')).toBeInTheDocument();
  });

  it('shows PR badge with noopener noreferrer', () => {
    renderCard(makeSession({ metadata: { pr_url: 'https://github.com/a/b/pull/1' } }));
    const badge = screen.getByTestId('pr-badge');
    expect(badge).toHaveAttribute('rel', 'noopener noreferrer');
  });

  it('shows workdir full path when no slash', () => {
    renderCard(makeSession({ workdir: 'relative-dir' }));
    expect(screen.getByTestId('session-workdir')).toHaveTextContent('relative-dir');
  });

  // Quick-reply bar

  it('shows quick-reply bar for idle session with output_snippet', () => {
    renderCard(
      makeSession({
        status: 'idle',
        output_snippet: 'Do you trust this file? (Y/N)',
      } as Partial<Session>),
    );
    expect(screen.getByTestId('quick-reply-bar')).toBeInTheDocument();
    expect(screen.getByTestId('quick-reply-yes')).toBeInTheDocument();
    expect(screen.getByTestId('quick-reply-no')).toBeInTheDocument();
    expect(screen.getByTestId('quick-reply-1')).toBeInTheDocument();
  });

  it('shows output_snippet in subtitle for idle sessions', () => {
    renderCard(
      makeSession({
        status: 'idle',
        output_snippet: 'Building...\nDo you trust this file?',
      } as Partial<Session>),
    );
    expect(screen.getByTestId('idle-snippet')).toBeInTheDocument();
    expect(screen.getByTestId('idle-snippet')).toHaveTextContent('Do you trust this file?');
  });

  it('sends input when quick-reply button clicked', () => {
    mockSendInput.mockResolvedValue(undefined);
    const onRefresh = vi.fn();
    renderCard(
      makeSession({
        status: 'idle',
        output_snippet: 'Continue? [Y/n]',
      } as Partial<Session>),
      onRefresh,
    );
    fireEvent.click(screen.getByTestId('quick-reply-yes'));
    expect(mockSendInput).toHaveBeenCalledWith('sess-1', 'yes\n');
    expect(onRefresh).toHaveBeenCalled();
  });

  it('does not show quick-reply bar for active sessions', () => {
    renderCard(makeSession({ status: 'active' }));
    expect(screen.queryByTestId('quick-reply-bar')).not.toBeInTheDocument();
  });

  it('does not show quick-reply bar for idle sessions without output_snippet', () => {
    renderCard(makeSession({ status: 'idle' }));
    expect(screen.queryByTestId('quick-reply-bar')).not.toBeInTheDocument();
  });

  // View toggle for non-active

  it('does not show view toggle for ready sessions', () => {
    renderCard(makeSession({ status: 'ready' }));
    clickExpand();
    expect(screen.queryByTestId('btn-view-toggle')).not.toBeInTheDocument();
  });

  it('loads intervention history on toggle', async () => {
    mockGetInterventionEvents.mockResolvedValue([
      { id: 1, session_id: 'sess-1', reason: 'OOM kill', created_at: '2026-01-01T12:00:00Z' },
    ]);
    renderCard(
      makeSession({
        status: 'stopped',
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

  // Git branch badge

  it('shows git branch badge when git_branch is set and no worktree', () => {
    renderCard(makeSession({ git_branch: 'main', git_commit: 'abc1234' }));
    const badge = screen.getByTestId('git-branch-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('main');
    expect(badge).toHaveTextContent('@abc1234');
  });

  it('shows git branch badge without commit when git_commit is null', () => {
    renderCard(makeSession({ git_branch: 'develop' }));
    const badge = screen.getByTestId('git-branch-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('develop');
    expect(badge).not.toHaveTextContent('@');
  });

  it('hides git branch badge when git_branch is null', () => {
    renderCard(makeSession());
    expect(screen.queryByTestId('git-branch-badge')).not.toBeInTheDocument();
  });

  it('hides git branch badge when worktree_path is set', () => {
    renderCard(
      makeSession({
        worktree_path: '/repo/.pulpo/worktrees/my-task',
        git_branch: 'my-task',
      }),
    );
    expect(screen.queryByTestId('git-branch-badge')).not.toBeInTheDocument();
    expect(screen.getByTestId('worktree-badge')).toBeInTheDocument();
  });
});
