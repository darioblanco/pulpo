import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { NewSessionDialog } from './new-session-dialog';
import * as api from '@/api/client';
import type { PeerInfo } from '@/api/types';

vi.mock('@/api/client', () => ({
  createSession: vi.fn(),
  createRemoteSession: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockCreateSession = vi.mocked(api.createSession);
const mockCreateRemoteSession = vi.mocked(api.createRemoteSession);

beforeEach(() => {
  mockCreateSession.mockReset();
  mockCreateRemoteSession.mockReset();
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
    mockCreateSession.mockResolvedValue({
      id: '1',
      name: 'test',
      provider: 'claude',
      status: 'creating',
      prompt: 'Fix',
      mode: 'interactive',
      workdir: '/repo',
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
    });
    const sessionResult = {
      id: '1',
      name: 'test',
      provider: 'claude',
      status: 'creating',
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
    };
    mockCreateSession.mockResolvedValue(sessionResult);
    const onCreated = vi.fn();
    render(<NewSessionDialog onCreated={onCreated} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Working directory'), '/home/user/repo');
    await user.type(screen.getByLabelText('Prompt'), 'Fix the bug');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith({
        workdir: '/home/user/repo',
        prompt: 'Fix the bug',
        provider: 'claude',
        mode: 'interactive',
        guard_preset: 'standard',
      });
      expect(onCreated).toHaveBeenCalledWith(sessionResult);
    });
  });

  it('sends name when provided', async () => {
    mockCreateSession.mockResolvedValue({
      id: '1',
      name: 'my-task',
      provider: 'claude',
      status: 'creating',
      prompt: 'Fix',
      mode: 'interactive',
      workdir: '/repo',
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
    });
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Name'), 'my-task');
    await user.type(screen.getByLabelText('Working directory'), '/repo');
    await user.type(screen.getByLabelText('Prompt'), 'Fix it');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(mockCreateSession).toHaveBeenCalledWith({
        name: 'my-task',
        workdir: '/repo',
        prompt: 'Fix it',
        provider: 'claude',
        mode: 'interactive',
        guard_preset: 'standard',
      });
    });
  });

  it('shows error on failed submission', async () => {
    mockCreateSession.mockRejectedValue(new Error('Network error'));
    render(<NewSessionDialog onCreated={vi.fn()} />);
    const user = await openDialog();

    await user.type(screen.getByLabelText('Working directory'), '/repo');
    await user.type(screen.getByLabelText('Prompt'), 'Test');

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

    await user.type(screen.getByLabelText('Working directory'), '/repo');
    await user.type(screen.getByLabelText('Prompt'), 'Test');

    const form = screen.getByLabelText('Working directory').closest('form')!;
    fireEvent.submit(form);

    await waitFor(() => {
      expect(screen.getByText('Failed to create session')).toBeInTheDocument();
    });
  });

  it('calls createRemoteSession for remote target', async () => {
    mockCreateRemoteSession.mockResolvedValue({
      id: '2',
      name: 'remote-test',
      provider: 'claude',
      status: 'creating',
      prompt: 'Fix',
      mode: 'interactive',
      workdir: '/repo',
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

    await user.type(screen.getByLabelText('Working directory'), '/repo');
    await user.type(screen.getByLabelText('Prompt'), 'Fix it');

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
        workdir: '/repo',
        prompt: 'Fix it',
        provider: 'claude',
        mode: 'interactive',
        guard_preset: 'standard',
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
});
