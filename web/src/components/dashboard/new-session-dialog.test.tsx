import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { NewSessionDialog } from './new-session-dialog';
import * as api from '@/api/client';

vi.mock('@/api/client', () => ({
  createSession: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockCreateSession = vi.mocked(api.createSession);

const defaultSession = {
  id: '1',
  name: 'test',
  status: 'creating',
  command: 'claude code',
  description: null,
  workdir: '/repo',
  metadata: null,
  ink: null,
  intervention_reason: null,
  intervention_at: null,
  last_output_at: null,

  created_at: '2025-01-01T00:00:00Z',
};

beforeEach(() => {
  mockCreateSession.mockReset();
});

async function openDialog() {
  const user = userEvent.setup({ pointerEventsCheck: 0 });
  await user.click(screen.getByTestId('new-session-button'));
  return user;
}

describe('NewSessionDialog', () => {
  it('renders the trigger button', () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    expect(screen.getByTestId('new-session-button')).toBeInTheDocument();
  });

  it('opens dialog on button click', async () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();
    expect(screen.getByText('Create New Session')).toBeInTheDocument();
  });

  it('submit button is disabled when fields are empty', async () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();
    const submit = screen.getByText('Create Session');
    expect(submit).toBeDisabled();
  });

  it('calls createSession for local target on submit', async () => {
    const sessionResult = { ...defaultSession, command: 'claude code', workdir: '/home/user/repo' };
    mockCreateSession.mockResolvedValue({ session: sessionResult });
    const onCreated = vi.fn();
    render(<NewSessionDialog onCreated={onCreated} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'my-session');
    await user.type(screen.getByLabelText('Working directory'), '/home/user/repo');
    await user.type(screen.getByLabelText('Command'), 'claude code');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith({
        name: 'my-session',
        workdir: '/home/user/repo',
        command: 'claude code',
      });
      expect(onCreated).toHaveBeenCalledWith(sessionResult);
    });
  });

  it('sends name when provided', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession, name: 'my-task' } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'my-task');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith({
        name: 'my-task',
        workdir: '/repo',
      });
    });
  });

  it('shows error on failed submission', async () => {
    mockCreateSession.mockRejectedValue(new Error('Network error'));
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'err-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(screen.getByText('Network error')).toBeInTheDocument();
    });
  });

  it('shows non-Error failure message', async () => {
    mockCreateSession.mockRejectedValue('string error');
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'str-err');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(screen.getByText('Failed to create session')).toBeInTheDocument();
    });
  });

  it('shows worktree toggle in dialog', async () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();
    expect(screen.getByLabelText(/Worktree/)).toBeInTheDocument();
  });

  it('shows helper text when worktree is enabled', async () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();
    const toggle = screen.getByRole('switch');
    await user.click(toggle);
    expect(screen.getByText('Run in an isolated git worktree')).toBeInTheDocument();
  });

  it('sends worktree flag when toggle is enabled', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'wt-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const toggle = screen.getByRole('switch');
    await user.click(toggle);

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith(
        expect.objectContaining({
          name: 'wt-test',
          workdir: '/repo',
          worktree: true,
        }),
      );
    });
  });

  it('does not send worktree flag when toggle is off', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'no-wt');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith({
        name: 'no-wt',
        workdir: '/repo',
      });
    });
  });

  it('sends description when provided', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'desc-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');
    await user.type(screen.getByLabelText('Description'), 'My task description');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith(
        expect.objectContaining({
          name: 'desc-test',
          workdir: '/repo',
          description: 'My task description',
        }),
      );
    });
  });

  it('shows worktree base field when worktree is enabled', async () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();
    expect(screen.queryByTestId('worktree-base-field')).not.toBeInTheDocument();
    const toggle = screen.getByRole('switch');
    await user.click(toggle);
    expect(screen.getByTestId('worktree-base-field')).toBeInTheDocument();
    expect(screen.getByLabelText('Base Branch')).toBeInTheDocument();
  });

  it('sends worktree_base when worktree is enabled and base is set', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'wt-base-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const toggle = screen.getByRole('switch');
    await user.click(toggle);
    await user.type(screen.getByLabelText('Base Branch'), 'develop');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith(
        expect.objectContaining({
          name: 'wt-base-test',
          workdir: '/repo',
          worktree: true,
          worktree_base: 'develop',
        }),
      );
    });
  });

  it('does not send worktree_base when worktree is off', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'no-wt-base');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith({
        name: 'no-wt-base',
        workdir: '/repo',
      });
    });
  });

  it('does not show a runtime selector (docker runtime removed)', async () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();
    expect(screen.queryByLabelText('Runtime')).not.toBeInTheDocument();
  });

  it('never sends a runtime field', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'tmux-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      const call = mockCreateSession.mock.calls[0][0];
      expect(call).not.toHaveProperty('runtime');
    });
  });

  it('shows idle threshold field in dialog', async () => {
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();
    expect(screen.getByLabelText('Idle Threshold (seconds)')).toBeInTheDocument();
  });

  it('sends idle_threshold_secs when set', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'idle-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');
    await user.type(screen.getByLabelText('Idle Threshold (seconds)'), '120');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith(
        expect.objectContaining({
          name: 'idle-test',
          workdir: '/repo',
          idle_threshold_secs: 120,
        }),
      );
    });
  });

  it('does not send idle_threshold_secs when empty', async () => {
    mockCreateSession.mockResolvedValue({ session: { ...defaultSession } });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'no-idle');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      const call = mockCreateSession.mock.calls[0][0];
      expect(call).not.toHaveProperty('idle_threshold_secs');
    });
  });
});
