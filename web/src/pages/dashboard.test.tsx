import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SidebarProvider } from '@/components/ui/sidebar';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { setApiConfig } from '@/api/client';
import { DashboardPage } from './dashboard';

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

class MockEventSource {
  url: string;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners: Record<string, ((e: { data: string }) => void)[]> = {};

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  addEventListener(type: string, handler: (e: { data: string }) => void) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(handler);
  }

  close() {}

  static instances: MockEventSource[] = [];
  static reset() {
    MockEventSource.instances = [];
  }
}

vi.stubGlobal('EventSource', MockEventSource);

// Mock Notification
vi.stubGlobal('Notification', { permission: 'default', requestPermission: vi.fn() });

beforeEach(() => {
  MockEventSource.reset();
  mockFetch.mockReset();
  setApiConfig({ getBaseUrl: () => '', getAuthToken: () => '' });
});

function renderDashboard() {
  // Default mock: getPeers and getSessions both return data
  mockFetch.mockImplementation(async (url: string) => {
    if (url.includes('/peers')) {
      return {
        ok: true,
        json: () =>
          Promise.resolve({
            local: {
              name: 'mac-studio',
              hostname: 'mac-studio.local',
              os: 'macOS',
              arch: 'arm64',
              cpus: 12,
              memory_mb: 65536,
              gpu: null,
            },
            peers: [],
          }),
      };
    }
    // SSE hydration and other calls
    return { ok: true, json: () => Promise.resolve([]) };
  });

  return render(
    <MemoryRouter>
      <ConnectionProvider>
        <SSEProvider>
          <TooltipProvider>
            <SidebarProvider>
              <DashboardPage />
            </SidebarProvider>
          </TooltipProvider>
        </SSEProvider>
      </ConnectionProvider>
    </MemoryRouter>,
  );
}

describe('DashboardPage', () => {
  it('renders the dashboard', () => {
    renderDashboard();
    expect(screen.getByTestId('dashboard-page')).toBeInTheDocument();
  });

  it('shows loading skeleton initially', () => {
    renderDashboard();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('shows node card after data loads', async () => {
    renderDashboard();
    await waitFor(() => {
      expect(screen.getByText('mac-studio')).toBeInTheDocument();
    });
  });

  it('shows status summary and new session button after data loads', async () => {
    renderDashboard();
    await waitFor(() => {
      expect(screen.getByTestId('status-summary')).toBeInTheDocument();
      expect(screen.getByTestId('new-session-button')).toBeInTheDocument();
    });
  });

  it('does not show tabs for single node', async () => {
    renderDashboard();
    await waitFor(() => {
      expect(screen.getByText('mac-studio')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('node-tabs')).not.toBeInTheDocument();
  });

  it('renders tabs when peers are present', async () => {
    mockFetch.mockImplementation(async (url: string) => {
      if (url.includes('/peers')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              local: {
                name: 'mac-studio',
                hostname: 'mac-studio.local',
                os: 'macOS',
                arch: 'arm64',
                cpus: 12,
                memory_mb: 65536,
                gpu: null,
              },
              peers: [
                {
                  name: 'remote-node',
                  address: 'remote:7433',
                  status: 'online',
                  node_info: {
                    name: 'remote-node',
                    hostname: 'remote.local',
                    os: 'Linux',
                    arch: 'x86_64',
                    cpus: 8,
                    memory_mb: 32768,
                    gpu: null,
                  },
                  session_count: null,
                },
              ],
            }),
        };
      }
      return { ok: true, json: () => Promise.resolve([]) };
    });

    render(
      <MemoryRouter>
        <ConnectionProvider>
          <SSEProvider>
            <TooltipProvider>
              <SidebarProvider>
                <DashboardPage />
              </SidebarProvider>
            </TooltipProvider>
          </SSEProvider>
        </ConnectionProvider>
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('node-tabs')).toBeInTheDocument();
      expect(screen.getByTestId('tab-local')).toBeInTheDocument();
      expect(screen.getByTestId('tab-remote-node')).toBeInTheDocument();
    });
  });

  it('handles remote session fetch failure gracefully', async () => {
    mockFetch.mockImplementation(async (url: string) => {
      if (url.includes('/peers')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              local: {
                name: 'mac-studio',
                hostname: 'mac-studio.local',
                os: 'macOS',
                arch: 'arm64',
                cpus: 12,
                memory_mb: 65536,
                gpu: null,
              },
              peers: [
                {
                  name: 'failing-peer',
                  address: 'failing:7433',
                  status: 'online',
                  node_info: null,
                  session_count: null,
                },
              ],
            }),
        };
      }
      // getRemoteSessions for the peer will fail
      throw new Error('Connection refused');
    });

    render(
      <MemoryRouter>
        <ConnectionProvider>
          <SSEProvider>
            <TooltipProvider>
              <SidebarProvider>
                <DashboardPage />
              </SidebarProvider>
            </TooltipProvider>
          </SSEProvider>
        </ConnectionProvider>
      </MemoryRouter>,
    );

    await waitFor(() => {
      // Peer tab should still render despite session fetch failure
      expect(screen.getByTestId('tab-failing-peer')).toBeInTheDocument();
    });
  });

  it('shows active sessions on local node and filters by status', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'running-task',
        provider: 'claude',
        status: 'running',
        prompt: 'Fix',
        mode: 'interactive',
        workdir: '/repo',
        guard_config: null,
        model: null,
        allowed_tools: null,
        system_prompt: null,
        metadata: null,
        ink: null,
        max_turns: null,
        max_budget_usd: null,
        output_format: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,
        waiting_for_input: false,
        created_at: '2025-01-01T00:00:00Z',
      },
      {
        id: 'sess-2',
        name: 'done-task',
        provider: 'claude',
        status: 'completed',
        prompt: 'Done',
        mode: 'interactive',
        workdir: '/repo',
        guard_config: null,
        model: null,
        allowed_tools: null,
        system_prompt: null,
        metadata: null,
        ink: null,
        max_turns: null,
        max_budget_usd: null,
        output_format: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,
        waiting_for_input: false,
        created_at: '2025-01-01T00:00:00Z',
      },
    ];

    mockFetch.mockImplementation(async (url: string) => {
      if (url.includes('/peers')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              local: {
                name: 'mac-studio',
                hostname: 'mac-studio.local',
                os: 'macOS',
                arch: 'arm64',
                cpus: 12,
                memory_mb: 65536,
                gpu: null,
              },
              peers: [],
            }),
        };
      }
      if (url.includes('/sessions')) {
        return { ok: true, json: () => Promise.resolve(sessionData) };
      }
      return { ok: true, json: () => Promise.resolve([]) };
    });

    render(
      <MemoryRouter>
        <ConnectionProvider>
          <SSEProvider>
            <TooltipProvider>
              <SidebarProvider>
                <DashboardPage />
              </SidebarProvider>
            </TooltipProvider>
          </SSEProvider>
        </ConnectionProvider>
      </MemoryRouter>,
    );

    // Trigger SSE onopen to hydrate sessions
    await waitFor(() => expect(MockEventSource.instances.length).toBeGreaterThan(0));
    const es = MockEventSource.instances[0];
    es.onopen?.();

    // Active sessions should be filtered (only running, creating, stale)
    await waitFor(() => {
      expect(screen.getByTestId('count-running').textContent).toBe('1');
    });
    // Only the running session shows on the local node card
    expect(screen.getByText('running-task')).toBeInTheDocument();
  });

  it('processes notifications when SSE delivers a status change', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'my-task',
        provider: 'claude',
        status: 'running',
        prompt: 'Fix',
        mode: 'interactive',
        workdir: '/repo',
        guard_config: null,
        model: null,
        allowed_tools: null,
        system_prompt: null,
        metadata: null,
        ink: null,
        max_turns: null,
        max_budget_usd: null,
        output_format: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,
        waiting_for_input: false,
        created_at: '2025-01-01T00:00:00Z',
      },
    ];

    mockFetch.mockImplementation(async (url: string) => {
      if (url.includes('/peers')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              local: {
                name: 'mac-studio',
                hostname: 'mac-studio.local',
                os: 'macOS',
                arch: 'arm64',
                cpus: 12,
                memory_mb: 65536,
                gpu: null,
              },
              peers: [],
            }),
        };
      }
      if (url.includes('/sessions')) {
        return { ok: true, json: () => Promise.resolve(sessionData) };
      }
      return { ok: true, json: () => Promise.resolve([]) };
    });

    render(
      <MemoryRouter>
        <ConnectionProvider>
          <SSEProvider>
            <TooltipProvider>
              <SidebarProvider>
                <DashboardPage />
              </SidebarProvider>
            </TooltipProvider>
          </SSEProvider>
        </ConnectionProvider>
      </MemoryRouter>,
    );

    // Trigger SSE connection and hydrate
    await waitFor(() => expect(MockEventSource.instances.length).toBeGreaterThan(0));
    const es = MockEventSource.instances[0];
    es.onopen?.();

    // Wait for initial sessions to load
    await waitFor(() => {
      expect(screen.getByTestId('count-running').textContent).toBe('1');
    });

    // Now send a session event changing status to completed
    const sessionHandler = es.listeners['session']?.[0];
    expect(sessionHandler).toBeDefined();
    sessionHandler({
      data: JSON.stringify({
        session_id: 'sess-1',
        session_name: 'my-task',
        status: 'completed',
        output_snippet: null,
        waiting_for_input: null,
      }),
    });

    // The notification processing should fire (previousRef has length > 0)
    await waitFor(() => {
      expect(screen.getByTestId('count-completed').textContent).toBe('1');
    });
  });

  it('shows error when fetch fails and connected', async () => {
    // Must have an active URL so isConnected = true (avoids redirect to /connect)
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => (key === 'pulpo:activeUrl' ? 'http://localhost:7433' : null),
      setItem: () => {},
      removeItem: () => {},
    });
    mockFetch.mockRejectedValue(new Error('Network error'));
    render(
      <MemoryRouter>
        <ConnectionProvider>
          <SSEProvider>
            <TooltipProvider>
              <SidebarProvider>
                <DashboardPage />
              </SidebarProvider>
            </TooltipProvider>
          </SSEProvider>
        </ConnectionProvider>
      </MemoryRouter>,
    );
    await waitFor(() => {
      expect(screen.getByText('Failed to connect to pulpod')).toBeInTheDocument();
    });
    // Restore default mock
    vi.stubGlobal('localStorage', {
      getItem: () => null,
      setItem: () => {},
      removeItem: () => {},
    });
  });
});
