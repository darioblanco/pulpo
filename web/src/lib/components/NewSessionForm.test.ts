import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent } from '@testing-library/svelte';
import NewSessionForm from './NewSessionForm.svelte';
import * as api from '$lib/api';

vi.mock('$lib/api', () => ({
  createSession: vi.fn(),
  createRemoteSession: vi.fn(),
}));

const mockCreateSession = vi.mocked(api.createSession);
const mockCreateRemoteSession = vi.mocked(api.createRemoteSession);

afterEach(cleanup);

beforeEach(() => {
  mockCreateSession.mockReset();
  mockCreateRemoteSession.mockReset();
});

describe('NewSessionForm', () => {
  it('renders all form fields with defaults', () => {
    render(NewSessionForm, {
      props: { oncreated: vi.fn() },
    });

    expect(screen.getByLabelText('Working directory')).toBeTruthy();
    expect(screen.getByLabelText('Prompt')).toBeTruthy();
    expect(screen.getByLabelText('Provider')).toBeTruthy();
    expect(screen.getByLabelText('Mode')).toBeTruthy();
    expect(screen.getByLabelText('Guards')).toBeTruthy();
    expect(screen.getByLabelText('Node')).toBeTruthy();
    expect(screen.getByText('Create Session')).toBeTruthy();
  });

  it('submit button is disabled when fields are empty', () => {
    render(NewSessionForm, {
      props: { oncreated: vi.fn() },
    });

    const button = screen.getByText('Create Session') as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });

  it('calls createSession for local target on submit', async () => {
    mockCreateSession.mockResolvedValue({
      id: '1',
      name: 'test',
      provider: 'claude',
      status: 'creating',
      prompt: 'Fix the bug',
      mode: 'interactive',
      workdir: '/repo',
      guard_config: null,
      intervention_reason: null,
      intervention_at: null,
      recovery_count: 0,
      last_output_at: null,
      created_at: '2025-01-01T00:00:00Z',
    });
    const oncreated = vi.fn();
    render(NewSessionForm, {
      props: { oncreated },
    });

    const repoInput = screen.getByLabelText('Working directory') as HTMLInputElement;
    const promptInput = screen.getByLabelText('Prompt') as HTMLTextAreaElement;

    await fireEvent.input(repoInput, { target: { value: '/home/user/repo' } });
    await fireEvent.input(promptInput, { target: { value: 'Fix the bug' } });

    const form = repoInput.closest('form')!;
    await fireEvent.submit(form);

    expect(mockCreateSession).toHaveBeenCalledWith({
      workdir: '/home/user/repo',
      prompt: 'Fix the bug',
      provider: 'claude',
      mode: 'interactive',
      guard_preset: 'standard',
    });
  });

  it('submits with changed provider, mode, and guard preset', async () => {
    mockCreateSession.mockResolvedValue({
      id: '1',
      name: 'test',
      provider: 'codex',
      status: 'creating',
      prompt: 'Do stuff',
      mode: 'autonomous',
      workdir: '/repo',
      guard_config: null,
      intervention_reason: null,
      intervention_at: null,
      recovery_count: 0,
      last_output_at: null,
      created_at: '2025-01-01T00:00:00Z',
    });
    render(NewSessionForm, {
      props: { oncreated: vi.fn() },
    });

    const repoInput = screen.getByLabelText('Working directory') as HTMLInputElement;
    const promptInput = screen.getByLabelText('Prompt') as HTMLTextAreaElement;
    const providerSelect = screen.getByLabelText('Provider') as HTMLSelectElement;
    const modeSelect = screen.getByLabelText('Mode') as HTMLSelectElement;
    const guardSelect = screen.getByLabelText('Guards') as HTMLSelectElement;

    await fireEvent.input(repoInput, { target: { value: '/repo' } });
    await fireEvent.input(promptInput, { target: { value: 'Do stuff' } });
    await fireEvent.change(providerSelect, { target: { value: 'codex' } });
    await fireEvent.change(modeSelect, { target: { value: 'autonomous' } });
    await fireEvent.change(guardSelect, { target: { value: 'yolo' } });

    const form = repoInput.closest('form')!;
    await fireEvent.submit(form);

    expect(mockCreateSession).toHaveBeenCalledWith({
      workdir: '/repo',
      prompt: 'Do stuff',
      provider: 'codex',
      mode: 'autonomous',
      guard_preset: 'yolo',
    });
  });

  it('calls createRemoteSession for remote target', async () => {
    mockCreateRemoteSession.mockResolvedValue({
      id: '2',
      name: 'remote',
      provider: 'claude',
      status: 'creating',
      prompt: 'Do stuff',
      mode: 'interactive',
      workdir: '/repo',
      guard_config: null,
      intervention_reason: null,
      intervention_at: null,
      recovery_count: 0,
      last_output_at: null,
      created_at: '2025-01-01T00:00:00Z',
    });
    const oncreated = vi.fn();
    const peers: api.PeerInfo[] = [
      {
        name: 'win-pc',
        address: 'win-pc:7433',
        status: 'online',
        node_info: null,
        session_count: null,
      },
    ];
    render(NewSessionForm, {
      props: { oncreated, peers },
    });

    const repoInput = screen.getByLabelText('Working directory') as HTMLInputElement;
    const promptInput = screen.getByLabelText('Prompt') as HTMLTextAreaElement;
    const nodeSelect = screen.getByLabelText('Node') as HTMLSelectElement;

    await fireEvent.input(repoInput, { target: { value: '/repo' } });
    await fireEvent.input(promptInput, { target: { value: 'Do stuff' } });
    await fireEvent.change(nodeSelect, { target: { value: 'win-pc' } });

    const form = repoInput.closest('form')!;
    await fireEvent.submit(form);

    expect(mockCreateRemoteSession).toHaveBeenCalledWith('win-pc:7433', {
      workdir: '/repo',
      prompt: 'Do stuff',
      provider: 'claude',
      mode: 'interactive',
      guard_preset: 'standard',
    });
  });

  it('shows error on failed submission', async () => {
    mockCreateSession.mockRejectedValue(new Error('Network error'));
    render(NewSessionForm, {
      props: { oncreated: vi.fn() },
    });

    const repoInput = screen.getByLabelText('Working directory') as HTMLInputElement;
    const promptInput = screen.getByLabelText('Prompt') as HTMLTextAreaElement;

    await fireEvent.input(repoInput, { target: { value: '/repo' } });
    await fireEvent.input(promptInput, { target: { value: 'Do stuff' } });

    const form = repoInput.closest('form')!;
    await fireEvent.submit(form);

    // Wait for the error to appear
    await vi.waitFor(() => {
      expect(screen.getByText('Failed to create session')).toBeTruthy();
    });
  });

  it('only shows online peers in the node dropdown', () => {
    const peers: api.PeerInfo[] = [
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
    render(NewSessionForm, {
      props: { oncreated: vi.fn(), peers },
    });

    const nodeSelect = screen.getByLabelText('Node') as HTMLSelectElement;
    const options = Array.from(nodeSelect.options).map((o) => o.text);
    expect(options).toContain('Local');
    expect(options).toContain('online-peer');
    expect(options).not.toContain('offline-peer');
  });

  it('resets form and fires oncreated after successful creation', async () => {
    mockCreateSession.mockResolvedValue({
      id: '1',
      name: 'test',
      provider: 'claude',
      status: 'creating',
      prompt: 'Fix it',
      mode: 'interactive',
      workdir: '/repo',
      guard_config: null,
      intervention_reason: null,
      intervention_at: null,
      recovery_count: 0,
      last_output_at: null,
      created_at: '2025-01-01T00:00:00Z',
    });
    const oncreated = vi.fn();
    render(NewSessionForm, {
      props: { oncreated },
    });

    const repoInput = screen.getByLabelText('Working directory') as HTMLInputElement;
    const promptInput = screen.getByLabelText('Prompt') as HTMLTextAreaElement;

    await fireEvent.input(repoInput, { target: { value: '/repo' } });
    await fireEvent.input(promptInput, { target: { value: 'Fix it' } });

    const form = repoInput.closest('form')!;
    await fireEvent.submit(form);

    await vi.waitFor(() => {
      expect(oncreated).toHaveBeenCalled();
    });

    // Form should be reset
    expect(repoInput.value).toBe('');
    expect(promptInput.value).toBe('');
  });
});
