import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AttachModal } from './attach-modal';

vi.mock('@/api/client', () => ({
  getSessionOutput: vi.fn().mockResolvedValue({ output: 'Hello from session' }),
  sendInput: vi.fn(),
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

  it('renders the modal body', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="active"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('attach-modal-body')).toBeInTheDocument();
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
    expect(screen.queryByTestId('output-view')).not.toBeInTheDocument();
  });

  it('renders OutputView for killed sessions', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="killed"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('output-view')).toBeInTheDocument();
    expect(screen.queryByTestId('terminal-view')).not.toBeInTheDocument();
  });

  it('renders OutputView for lost sessions', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="lost"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('output-view')).toBeInTheDocument();
  });

  it('renders OutputView for finished sessions', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="finished"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('output-view')).toBeInTheDocument();
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
    expect(screen.getByText('Session terminal output for worker-alpha')).toBeInTheDocument();
  });
});
