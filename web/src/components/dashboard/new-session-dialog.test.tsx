import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { NewSessionDialog } from './new-session-dialog';
import * as api from '@/api/client';
import type { PeerInfo } from '@/api/types';

vi.mock('@/api/client', () => ({
  createSession: vi.fn(),
  createRemoteSession: vi.fn(),
  getInks: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockCreateSession = vi.mocked(api.createSession);
const mockCreateRemoteSession = vi.mocked(api.createRemoteSession);
const mockGetInks = vi.mocked(api.getInks);

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
  mockCreateRemoteSession.mockReset();
  mockGetInks.mockReset();
  mockGetInks.mockResolvedValue({ inks: {} });
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

  it('calls createRemoteSession for remote target', async () => {
    mockCreateRemoteSession.mockResolvedValue({
      session: { ...defaultSession, id: '2', name: 'remote-test' },
    });
    const peers: PeerInfo[] = [
      {
        name: 'remote-node',
        address: 'remote:7433',
        status: 'online',
        node_info: null,
        session_count: null,
      },
    ];
    const onCreated = vi.fn();
    render(<NewSessionDialog peers={peers} onCreated={onCreated} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'remote-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    // Select remote node
    const nodeSelect = screen.getByRole('combobox', { name: 'Node' });
    await user.click(nodeSelect);
    await waitFor(() => {
      expect(screen.getAllByText('remote-node').length).toBeGreaterThan(0);
    });
    // Click the option in the listbox
    const options = screen.getAllByText('remote-node');
    const listboxOption = options.find((el) => el.closest('[role="option"]'));
    if (listboxOption) await user.click(listboxOption);

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateRemoteSession).toHaveBeenCalledWith('remote:7433', {
        name: 'remote-test',
        workdir: '/repo',
      });
    });
  });

  it('only shows online peers in node selector', async () => {
    const peers: PeerInfo[] = [
      {
        name: 'online-peer',
        address: 'online:7433',
        status: 'online',
        node_info: null,
        session_count: null,
      },
      {
        name: 'offline-peer',
        address: 'offline:7433',
        status: 'offline',
        node_info: null,
        session_count: null,
      },
    ];
    render(<NewSessionDialog peers={peers} onCreated={vi.fn()} />);
    const user = await openDialog();

    // Click the node select trigger to open it
    const nodeSelect = screen.getByRole('combobox', { name: 'Node' });
    await user.click(nodeSelect);

    await waitFor(() => {
      expect(screen.getAllByText('online-peer').length).toBeGreaterThan(0);
    });
    expect(screen.queryByText('offline-peer')).not.toBeInTheDocument();
  });

  it('fetches inks when dialog opens', async () => {
    mockGetInks.mockResolvedValue({
      inks: {
        reviewer: {
          description: 'Code review',
          command: 'claude code --model opus-4',
        },
      },
    });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();

    await waitFor(() => {
      expect(mockGetInks).toHaveBeenCalled();
    });
  });

  it('shows ink selector when inks are available', async () => {
    mockGetInks.mockResolvedValue({
      inks: {
        reviewer: {
          description: 'Code review',
          command: 'claude code',
        },
      },
    });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();

    await waitFor(() => {
      expect(screen.getByLabelText('Ink')).toBeInTheDocument();
    });
  });

  it('does not show ink selector when no inks available', async () => {
    mockGetInks.mockResolvedValue({ inks: {} });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();

    // Wait for inks to load (empty)
    await waitFor(() => {
      expect(mockGetInks).toHaveBeenCalled();
    });
    expect(screen.queryByLabelText('Ink')).not.toBeInTheDocument();
  });

  it('auto-fills command when ink is selected', async () => {
    mockGetInks.mockResolvedValue({
      inks: {
        reviewer: {
          description: 'Code review',
          command: 'codex --model gpt-4o',
        },
      },
    });
    mockCreateSession.mockResolvedValue({
      session: { ...defaultSession, ink: 'reviewer', command: 'codex --model gpt-4o' },
    });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    // Wait for inks to load
    await waitFor(() => {
      expect(screen.getByLabelText('Ink')).toBeInTheDocument();
    });

    // Select the ink
    const inkSelect = screen.getByRole('combobox', { name: 'Ink' });
    await user.click(inkSelect);
    await waitFor(() => {
      expect(screen.getAllByText(/reviewer/).length).toBeGreaterThan(0);
    });
    const options = screen.getAllByText(/reviewer/);
    const listboxOption = options.find((el) => el.closest('[role="option"]'));
    if (listboxOption) await user.click(listboxOption);

    // Fill required fields and submit
    await user.type(screen.getByLabelText('Name'), 'ink-test');
    await user.type(screen.getByLabelText('Working directory'), '/repo');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith(
        expect.objectContaining({
          ink: 'reviewer',
          command: 'codex --model gpt-4o',
        }),
      );
    });
  });

  it('shows ink summary when ink is selected', async () => {
    mockGetInks.mockResolvedValue({
      inks: {
        reviewer: {
          description: 'Code review',
          command: 'codex --autonomous',
        },
      },
    });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await waitFor(() => {
      expect(screen.getByLabelText('Ink')).toBeInTheDocument();
    });

    // Select the ink
    const inkSelect = screen.getByRole('combobox', { name: 'Ink' });
    await user.click(inkSelect);
    await waitFor(() => {
      expect(screen.getAllByText(/reviewer/).length).toBeGreaterThan(0);
    });
    const options = screen.getAllByText(/reviewer/);
    const listboxOption = options.find((el) => el.closest('[role="option"]'));
    if (listboxOption) await user.click(listboxOption);

    await waitFor(() => {
      const summary = screen.getByTestId('ink-summary');
      expect(summary).toBeInTheDocument();
      expect(summary.textContent).toContain('codex');
    });
  });

  it('handles getInks failure gracefully', async () => {
    mockGetInks.mockRejectedValue(new Error('Network error'));
    render(<NewSessionDialog onCreated={vi.fn()} />);
    await openDialog();

    // Dialog should still work without ink selector
    expect(screen.getByText('Create New Session')).toBeInTheDocument();
    expect(screen.queryByLabelText('Ink')).not.toBeInTheDocument();
  });
});
