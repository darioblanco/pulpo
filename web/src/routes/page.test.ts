import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent } from '@testing-library/svelte';
import Page from './+page.svelte';
import * as api from '$lib/api';
import type { NodeInfo, PeersResponse, Session } from '$lib/api';

const mockGoto = vi.fn();

vi.mock('$app/navigation', () => ({
  goto: (...args: unknown[]) => mockGoto(...args),
}));

vi.mock('$lib/api', () => ({
  getPeers: vi.fn(),
  getSessions: vi.fn(),
  getRemoteSessions: vi.fn(),
  createSession: vi.fn(),
  createRemoteSession: vi.fn(),
  killSession: vi.fn(),
  getSessionOutput: vi.fn(),
  resumeSession: vi.fn(),
  sendInput: vi.fn(),
}));

// Mock the Terminal component — Svelte 5 components are functions
vi.mock('$lib/components/Terminal.svelte', () => ({
  default: function MockTerminal($$anchor: ChildNode) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'mock-terminal');
    $$anchor.before(el);
  },
}));

// Mock the ChatView component
vi.mock('$lib/components/ChatView.svelte', () => ({
  default: function MockChatView($$anchor: ChildNode) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'mock-chat-view');
    $$anchor.before(el);
  },
}));

const mockShowToast = vi.fn();
const mockShowDesktopNotification = vi.fn();

vi.mock('$lib/notifications', () => ({
  detectStatusChanges: vi.fn().mockReturnValue([]),
  showDesktopNotification: (...args: unknown[]) => mockShowDesktopNotification(...args),
}));

vi.mock('$lib/stores/notifications.svelte', () => ({
  showToast: (...args: unknown[]) => mockShowToast(...args),
  getToastMessage: vi.fn().mockReturnValue(''),
  isToastVisible: vi.fn().mockReturnValue(false),
}));

const mockIsConnected = vi.fn().mockReturnValue(false);

vi.mock('$lib/stores/connection.svelte', () => ({
  isConnected: () => mockIsConnected(),
  getBaseUrl: vi.fn().mockReturnValue(''),
  setBaseUrl: vi.fn(),
}));

const mockGetPeers = vi.mocked(api.getPeers);
const mockGetSessions = vi.mocked(api.getSessions);
const mockGetRemoteSessions = vi.mocked(api.getRemoteSessions);

function makeNodeInfo(overrides: Partial<NodeInfo> = {}): NodeInfo {
  return {
    name: 'mac-mini',
    hostname: 'mac-mini.local',
    os: 'macos',
    arch: 'aarch64',
    cpus: 10,
    memory_mb: 16384,
    gpu: null,
    ...overrides,
  };
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix the bug',
    mode: 'interactive',
    workdir: '/home/user/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    persona: null,
    intervention_reason: null,
    intervention_at: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    last_output_at: null,
    waiting_for_input: false,
    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

function makePeersResponse(
  peers: api.PeerInfo[] = [],
  local: NodeInfo = makeNodeInfo(),
): PeersResponse {
  return { local, peers };
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

beforeEach(() => {
  vi.useFakeTimers();
  mockGetPeers.mockReset();
  mockGetSessions.mockReset();
  mockGetRemoteSessions.mockReset();
  mockShowToast.mockReset();
  mockShowDesktopNotification.mockReset();
  mockGoto.mockReset();
  mockIsConnected.mockReturnValue(false);
});

describe('+page', () => {
  it('shows loading state before data arrives', () => {
    // Never resolve the promises
    mockGetPeers.mockReturnValue(new Promise(() => {}));

    render(Page);

    expect(screen.getByText('Connecting to pulpod...')).toBeTruthy();
  });

  it('redirects to /connect on API failure when not connected', async () => {
    mockGetPeers.mockRejectedValue(new Error('Connection refused'));
    mockIsConnected.mockReturnValue(false);

    render(Page);

    await vi.waitFor(() => {
      expect(mockGoto).toHaveBeenCalledWith('/connect');
    });
  });

  it('shows error message on API failure when already connected', async () => {
    mockGetPeers.mockRejectedValue(new Error('Connection refused'));
    mockIsConnected.mockReturnValue(true);

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Failed to connect to pulpod')).toBeTruthy();
    });
  });

  it('renders local NodeCard after data loads', async () => {
    mockGetPeers.mockResolvedValue(makePeersResponse());
    mockGetSessions.mockResolvedValue([makeSession()]);

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('mac-mini')).toBeTruthy();
    });

    expect(screen.getByText('local')).toBeTruthy();
    expect(screen.getByText('1 session')).toBeTruthy();
  });

  it('renders NodeCard for each peer', async () => {
    const peers: api.PeerInfo[] = [
      {
        name: 'win-pc',
        address: 'win-pc:7433',
        status: 'online',
        node_info: makeNodeInfo({ name: 'win-pc', os: 'linux' }),
        session_count: 2,
      },
      {
        name: 'macbook',
        address: 'macbook:7433',
        status: 'offline',
        node_info: null,
        session_count: null,
      },
    ];
    mockGetPeers.mockResolvedValue(makePeersResponse(peers));
    mockGetSessions.mockResolvedValue([]);
    mockGetRemoteSessions.mockResolvedValue([makeSession({ id: 'remote-1', name: 'remote-task' })]);

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('win-pc')).toBeTruthy();
    });

    expect(screen.getByText('macbook')).toBeTruthy();
  });

  it('toggles New Session form when button clicked', async () => {
    mockGetPeers.mockResolvedValue(makePeersResponse());
    mockGetSessions.mockResolvedValue([]);

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('+ New Session')).toBeTruthy();
    });

    const button = screen.getByText('+ New Session');
    await fireEvent.click(button);

    expect(screen.getByText('Cancel')).toBeTruthy();
    expect(screen.getByLabelText('Working directory')).toBeTruthy();

    // Click again to close
    const cancelBtn = screen.getByText('Cancel');
    await fireEvent.click(cancelBtn);

    expect(screen.getByText('+ New Session')).toBeTruthy();
  });

  it('handles remote peer session fetch failure gracefully', async () => {
    const peers: api.PeerInfo[] = [
      {
        name: 'flaky-peer',
        address: 'flaky:7433',
        status: 'online',
        node_info: makeNodeInfo({ name: 'flaky-peer' }),
        session_count: 0,
      },
    ];
    mockGetPeers.mockResolvedValue(makePeersResponse(peers));
    mockGetSessions.mockResolvedValue([]);
    mockGetRemoteSessions.mockRejectedValue(new Error('Connection refused'));

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('flaky-peer')).toBeTruthy();
    });

    // Should still render the peer card (with empty sessions from the catch)
    // Both local node and flaky-peer have 0 sessions
    expect(screen.getAllByText('0 sessions')).toHaveLength(2);
  });

  it('hides form and refreshes after session creation', async () => {
    const mockCreateSession = vi.mocked(api.createSession);
    mockCreateSession.mockResolvedValue({
      id: '1',
      name: 'test',
      provider: 'claude',
      status: 'creating',
      prompt: 'Do stuff',
      mode: 'interactive',
      workdir: '/repo',
      guard_config: null,
      created_at: '2025-01-01T00:00:00Z',
    });
    mockGetPeers.mockResolvedValue(makePeersResponse());
    mockGetSessions.mockResolvedValue([]);

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('+ New Session')).toBeTruthy();
    });

    // Open form
    await fireEvent.click(screen.getByText('+ New Session'));
    expect(screen.getByLabelText('Working directory')).toBeTruthy();

    // Fill form
    const repoInput = screen.getByLabelText('Working directory') as HTMLInputElement;
    const promptInput = screen.getByLabelText('Prompt') as HTMLTextAreaElement;
    await fireEvent.input(repoInput, { target: { value: '/repo' } });
    await fireEvent.input(promptInput, { target: { value: 'Do stuff' } });

    // Submit
    const form = repoInput.closest('form')!;
    await fireEvent.submit(form);

    // Form should close and data should refresh
    await vi.waitFor(() => {
      expect(screen.getByText('+ New Session')).toBeTruthy();
    });
  });

  it('cleans up polling interval on destroy', async () => {
    mockGetPeers.mockResolvedValue(makePeersResponse());
    mockGetSessions.mockResolvedValue([]);

    const { unmount } = render(Page);

    await vi.waitFor(() => {
      expect(mockGetPeers).toHaveBeenCalledTimes(1);
    });

    // Advance time to trigger polling
    await vi.advanceTimersByTimeAsync(5000);
    expect(mockGetPeers).toHaveBeenCalledTimes(2);

    unmount();

    // After unmount, advancing time should not trigger more calls
    const callCount = mockGetPeers.mock.calls.length;
    await vi.advanceTimersByTimeAsync(5000);
    expect(mockGetPeers).toHaveBeenCalledTimes(callCount);
  });

  it('fires notifications on session status changes', async () => {
    const { detectStatusChanges } = await import('$lib/notifications');
    const mockDetect = vi.mocked(detectStatusChanges);

    mockGetPeers.mockResolvedValue(makePeersResponse());
    mockGetSessions.mockResolvedValue([makeSession({ id: '1', status: 'running' })]);

    render(Page);

    // Wait for initial load (no notifications on first fetch)
    await vi.waitFor(() => {
      expect(mockGetSessions).toHaveBeenCalledTimes(1);
    });

    // Set up next poll to detect a change
    mockDetect.mockReturnValue([
      { sessionId: '1', sessionName: 'my-api', from: 'running', to: 'completed' },
    ]);
    mockGetSessions.mockResolvedValue([makeSession({ id: '1', status: 'completed' })]);

    await vi.advanceTimersByTimeAsync(5000);

    await vi.waitFor(() => {
      expect(mockShowToast).toHaveBeenCalledWith('my-api completed');
      expect(mockShowDesktopNotification).toHaveBeenCalledWith({
        sessionId: '1',
        sessionName: 'my-api',
        from: 'running',
        to: 'completed',
      });
    });
  });

  it('only shows active sessions on dashboard', async () => {
    mockGetPeers.mockResolvedValue(makePeersResponse());
    mockGetSessions.mockResolvedValue([
      makeSession({ id: '1', name: 'active-task', status: 'running' }),
      makeSession({ id: '2', name: 'done-task', status: 'completed' }),
    ]);

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('1 session')).toBeTruthy();
    });
  });
});
