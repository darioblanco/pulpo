import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AttachModal } from './attach-modal';

vi.mock('@/api/client', () => ({
  getSessionOutput: vi.fn().mockResolvedValue({ output: 'Hello from session' }),
  sendInput: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

describe('AttachModal', () => {
  it('renders session name in header when open', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="running"
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
        sessionStatus="running"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('attach-modal')).toBeInTheDocument();
  });

  it('renders the output view body', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="running"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByTestId('attach-modal-body')).toBeInTheDocument();
  });

  it('embeds OutputView component', () => {
    render(
      <AttachModal
        sessionName="worker-alpha"
        sessionId="sess-1"
        sessionStatus="running"
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
        sessionStatus="running"
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
        sessionStatus="running"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    expect(screen.getByText('Session terminal output for worker-alpha')).toBeInTheDocument();
  });
});
