import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { AttachModal } from './attach-modal';

vi.mock('@/api/client', () => ({
  resumeSession: vi.fn().mockResolvedValue({ id: 'sess-1', status: 'active' }),
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

describe('AttachModal', () => {
  it('renders session name in header when open', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="active"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByText('worker-alpha')).toBeInTheDocument();
  });

  it('renders the modal container', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="active"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('attach-modal')).toBeInTheDocument();
  });

  it('renders TerminalView for active sessions', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="active"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('terminal-view')).toBeInTheDocument();
  });

  it('renders TerminalView for idle sessions', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="idle"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('terminal-view')).toBeInTheDocument();
  });

  it('resumes lost session then shows terminal', async () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="lost"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    // Shows resuming state first
    expect(screen.getByText('Resuming session…')).toBeInTheDocument();
    // After resume completes, shows terminal
    await waitFor(() => {
      expect(screen.getByTestId('terminal-view')).toBeInTheDocument();
    });
  });

  it('resumes ready session then shows terminal', async () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="ready"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(screen.getByTestId('terminal-view')).toBeInTheDocument();
    });
  });

  it('shows error when resume fails', async () => {
    const { resumeSession } = await import('@/api/client');
    vi.mocked(resumeSession).mockRejectedValueOnce(new Error('network error'));

    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="lost"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    await waitFor(() => {
      expect(screen.getByText(/Failed to resume/)).toBeInTheDocument();
    });
  });

  it('does not render when closed', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="active"
        open={false}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.queryByTestId('attach-modal')).not.toBeInTheDocument();
  });

  it('includes accessible description', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="active"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByText('Session terminal for worker-alpha')).toBeInTheDocument();
  });
});
