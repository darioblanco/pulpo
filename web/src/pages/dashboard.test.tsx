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
  // Default mock: getNode and getSessions both return data
  mockFetch.mockImplementation(async (url: string) => {
    if (url.includes('/node')) {
      return {
        ok: true,
        json: () =>
          Promise.resolve({
            name: 'mac-studio',
            hostname: 'mac-studio.local',
            os: 'macOS',
            arch: 'arm64',
            cpus: 12,
            memory_mb: 65536,
            gpu: null,
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
      expect(screen.getByTestId('status-chip-starting')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('status-chip-working')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('status-chip-waiting')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('status-chip-lost')).toHaveAttribute('aria-pressed', 'true');
      expect(screen.getByTestId('status-chip-done')).toHaveAttribute('aria-pressed', 'false');
    });
  });

  it('does not show tabs for single node', async () => {
    renderDashboard();
    await waitFor(() => {
      expect(screen.getByText('mac-studio')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('node-tabs')).not.toBeInTheDocument();
  });

  it('shows sessions filtered by default statuses on local node', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'running-task',
        status: 'working',
        status_reason: null,
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
        name: 'lost-task',
        status: 'lost',
        status_reason: null,
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
        id: 'sess-3',
        name: 'done-task',
        status: 'done',
        status_reason: 'exited',
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
      if (url.includes('/node')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              name: 'mac-studio',
              hostname: 'mac-studio.local',
              os: 'macOS',
              arch: 'arm64',
              cpus: 12,
              memory_mb: 65536,
              gpu: null,
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

    // working and lost sessions show (default filters include starting, working,
    // waiting, lost — hides only done)
    await waitFor(() => {
      expect(screen.getByTestId('count-working').textContent).toBe('1');
      expect(screen.getByTestId('count-lost').textContent).toBe('1');
    });
    // working and lost sessions visible, done is hidden by default
    expect(screen.getByText('running-task')).toBeInTheDocument();
    expect(screen.getByText('lost-task')).toBeInTheDocument();
    expect(screen.queryByText('done-task')).not.toBeInTheDocument();
  });

  it('processes notifications when SSE delivers a status change', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'my-task',
        status: 'working',
        status_reason: null,
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
      if (url.includes('/node')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              name: 'mac-studio',
              hostname: 'mac-studio.local',
              os: 'macOS',
              arch: 'arm64',
              cpus: 12,
              memory_mb: 65536,
              gpu: null,
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
      expect(screen.getByTestId('count-working').textContent).toBe('1');
    });

    // Now send a session event changing status to done (clean exit)
    const sessionHandler = es.listeners['session']?.[0];
    expect(sessionHandler).toBeDefined();
    sessionHandler({
      data: JSON.stringify({
        session_id: 'sess-1',
        session_name: 'my-task',
        status: 'done',
        status_reason: 'exited',
        output_snippet: null,
      }),
    });

    // The notification processing should fire (previousRef has length > 0)
    await waitFor(() => {
      expect(screen.getByTestId('count-done').textContent).toBe('1');
    });
  });

  it('shows cleanup button when done sessions exist', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'done-task',
        status: 'done',
        status_reason: 'stopped',
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
      if (url.includes('/node')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              name: 'mac-studio',
              hostname: 'mac-studio.local',
              os: 'macOS',
              arch: 'arm64',
              cpus: 12,
              memory_mb: 65536,
              gpu: null,
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

  it('does not show cleanup button when no done/lost sessions', async () => {
    const sessionData = [
      {
        id: 'sess-1',
        name: 'working-task',
        status: 'working',
        status_reason: null,
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
      if (url.includes('/node')) {
        return {
          ok: true,
          json: () =>
            Promise.resolve({
              name: 'mac-studio',
              hostname: 'mac-studio.local',
              os: 'macOS',
              arch: 'arm64',
              cpus: 12,
              memory_mb: 65536,
              gpu: null,
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
