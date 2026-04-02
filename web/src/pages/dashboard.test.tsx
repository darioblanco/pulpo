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

  it('shows status summary, session filter, and new session button after data loads', async () => {
    renderDashboard();
    await waitFor(() => {
      expect(screen.getByTestId('status-summary')).toBeInTheDocument();
      expect(screen.getByTestId('session-filter')).toBeInTheDocument();
      expect(screen.getByTestId('new-session-button')).toBeInTheDocument();
    });
  });

  it('renders status filter chips with defaults selected', async () => {
    renderDashboard();
    await waitFor(() => {
      expect(screen.getByTestId('status-chip-active')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('status-chip-idle')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('status-chip-ready')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('status-chip-stopped')).toHaveAttribute('aria-pressed', 'false');
      expect(screen.getByTestId('status-chip-lost')).toHaveAttribute('aria-pressed', 'false');
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
              role: 'controller',
            }),
        };
      }
      if (url.includes('/fleet/sessions')) {
        return { ok: true, json: () => Promise.resolve({ sessions: [] }) };
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
      expect(screen.getByTestId('tab-all')).toBeInTheDocument();
      expect(screen.getByTestId('tab-local')).toBeInTheDocument();
      expect(screen.getByTestId('tab-remote-node')).toBeInTheDocument();
    });

    // Verify hardware subtitles on tabs
    const localSubtitle = screen.getByTestId('tab-local-subtitle');
    expect(localSubtitle).toHaveTextContent('macOS · 12 CPU · 64 GB');

    const peerSubtitle = screen.getByTestId('tab-remote-node-subtitle');
    expect(peerSubtitle).toHaveTextContent('Linux · 8 CPU · 32 GB');
  });

  it('handles peer with no sessions gracefully', async () => {
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
              role: 'controller',
            }),
        };
      }
      if (url.includes('/fleet/sessions')) {
        return { ok: true, json: () => Promise.resolve({ sessions: [] }) };
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
      // Peer tab should still render despite session fetch failure
      expect(screen.getByTestId('tab-failing-peer')).toBeInTheDocument();
    });
  });

  it('shows sessions filtered by default statuses on local node', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'running-task',
        status: 'active',
        command: 'Fix',
        description: null,
        workdir: '/repo',
        metadata: null,
        ink: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,

        created_at: '2025-01-01T00:00:00Z',
      },
      {
        id: 'sess-2',
        name: 'done-task',
        status: 'ready',
        command: 'Done',
        description: null,
        workdir: '/repo',
        metadata: null,
        ink: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,

        created_at: '2025-01-01T00:00:00Z',
      },
      {
        id: 'sess-3',
        name: 'stopped-task',
        status: 'stopped',
        command: 'Old',
        description: null,
        workdir: '/repo',
        metadata: null,
        ink: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,

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

    // Both active and ready sessions show (default filters include active, idle, ready)
    await waitFor(() => {
      expect(screen.getByTestId('count-active').textContent).toBe('1');
      expect(screen.getByTestId('count-ready').textContent).toBe('1');
    });
    // Active and ready sessions visible, stopped is hidden by default
    expect(screen.getByText('running-task')).toBeInTheDocument();
    expect(screen.getByText('done-task')).toBeInTheDocument();
    expect(screen.queryByText('stopped-task')).not.toBeInTheDocument();
  });

  it('processes notifications when SSE delivers a status change', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'my-task',
        status: 'active',
        command: 'Fix',
        description: null,
        workdir: '/repo',
        metadata: null,
        ink: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,

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
      expect(screen.getByTestId('count-active').textContent).toBe('1');
    });

    // Now send a session event changing status to ready
    const sessionHandler = es.listeners['session']?.[0];
    expect(sessionHandler).toBeDefined();
    sessionHandler({
      data: JSON.stringify({
        session_id: 'sess-1',
        session_name: 'my-task',
        status: 'ready',
        output_snippet: null,
      }),
    });

    // The notification processing should fire (previousRef has length > 0)
    await waitFor(() => {
      expect(screen.getByTestId('count-ready').textContent).toBe('1');
    });
  });

  it('shows fleet sessions in the All tab when peers are present', async () => {
    mockFetch.mockImplementation(async (url: string) => {
      if (url.includes('/fleet/sessions')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              sessions: [
                {
                  node_name: 'mac-studio',
                  node_address: '',
                  id: 'fleet-1',
                  name: 'local-task',
                  status: 'active',
                  command: 'claude code',
                  description: null,
                  workdir: '/repo',
                  metadata: null,
                  ink: null,
                  idle_threshold_secs: null,
                  intervention_reason: null,
                  intervention_at: null,
                  last_output_at: null,
                  created_at: '2025-01-01T00:00:00Z',
                },
                {
                  node_name: 'remote-node',
                  node_address: 'remote:7433',
                  id: 'fleet-2',
                  name: 'remote-task',
                  status: 'idle',
                  command: 'npm test',
                  description: null,
                  workdir: '/app',
                  metadata: null,
                  ink: null,
                  idle_threshold_secs: null,
                  intervention_reason: null,
                  intervention_at: null,
                  last_output_at: null,
                  created_at: '2025-01-01T00:00:00Z',
                },
              ],
              role: 'controller',
            }),
        };
      }
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
                  node_info: null,
                  session_count: null,
                },
              ],
              role: 'controller',
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
      expect(screen.getByTestId('fleet-table')).toBeInTheDocument();
    });

    // Both fleet sessions should be visible with node names
    expect(screen.getByText('local-task')).toBeInTheDocument();
    expect(screen.getByText('remote-task')).toBeInTheDocument();
    // mac-studio appears in both the tab trigger and the fleet table
    expect(screen.getAllByText('mac-studio').length).toBeGreaterThanOrEqual(2);
    // remote-node appears in both the tab trigger and the fleet table
    expect(screen.getAllByText('remote-node').length).toBeGreaterThanOrEqual(2);

    // The All tab should show the count
    const allTab = screen.getByTestId('tab-all');
    expect(allTab).toHaveTextContent('All');
    expect(allTab).toHaveTextContent('(2)');
  });

  it('shows empty fleet message when no matching fleet sessions', async () => {
    mockFetch.mockImplementation(async (url: string) => {
      if (url.includes('/fleet/sessions')) {
        return {
          ok: true,
          json: () => Promise.resolve({ sessions: [] }),
        };
      }
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
                  node_info: null,
                  session_count: null,
                },
              ],
              role: 'controller',
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
      expect(screen.getByText('No matching sessions across the fleet.')).toBeInTheDocument();
    });
  });

  it('handles fleet session fetch failure gracefully', async () => {
    mockFetch.mockImplementation(async (url: string) => {
      if (url.includes('/fleet/sessions')) {
        throw new Error('Fleet endpoint unavailable');
      }
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
                  node_info: null,
                  session_count: null,
                },
              ],
              role: 'controller',
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

    // Should still render the tabs without crashing
    await waitFor(() => {
      expect(screen.getByTestId('node-tabs')).toBeInTheDocument();
      expect(screen.getByTestId('tab-all')).toBeInTheDocument();
    });

    // Empty fleet = shows empty message
    expect(screen.getByText('No matching sessions across the fleet.')).toBeInTheDocument();
  });

  it('shows cleanup button when stopped sessions exist', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'stopped-task',
        status: 'stopped',
        command: 'done',
        description: null,
        workdir: '/repo',
        metadata: null,
        ink: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,
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
      if (url.includes('/sessions/cleanup')) {
        return { ok: true, json: () => Promise.resolve({ deleted: 1 }) };
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

    await waitFor(() => {
      expect(screen.getByTestId('cleanup-button')).toBeInTheDocument();
    });
  });

  it('does not show cleanup button when no stopped/lost sessions', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'active-task',
        status: 'active',
        command: 'run',
        description: null,
        workdir: '/repo',
        metadata: null,
        ink: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,
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

    await waitFor(() => expect(MockEventSource.instances.length).toBeGreaterThan(0));
    const es = MockEventSource.instances[0];
    es.onopen?.();

    await waitFor(() => {
      expect(screen.getByTestId('new-session-button')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('cleanup-button')).not.toBeInTheDocument();
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
