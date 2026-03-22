import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { SSEProvider, useSSE } from './use-sse';
import { ConnectionProvider } from './use-connection';
import { setApiConfig } from '@/api/client';
import type { ReactNode } from 'react';

// Mock localStorage
vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

// Mock fetch for hydration
const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

// Mock EventSource
class MockEventSource {
  url: string;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners: Record<string, ((e: { data: string }) => void)[]> = {};
  closed = false;

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  addEventListener(type: string, handler: (e: { data: string }) => void) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(handler);
  }

  close() {
    this.closed = true;
  }

  _fireEvent(type: string, data: string) {
    for (const handler of this.listeners[type] ?? []) {
      handler({ data });
    }
  }

  static instances: MockEventSource[] = [];
  static reset() {
    MockEventSource.instances = [];
  }
}

vi.stubGlobal('EventSource', MockEventSource);

let testUrl = '';
let testToken = '';

beforeEach(() => {
  MockEventSource.reset();
  mockFetch.mockReset();
  testUrl = '';
  testToken = '';
  setApiConfig({
    getBaseUrl: () => testUrl,
    getAuthToken: () => testToken,
  });
});

afterEach(() => {
  vi.useRealTimers();
});

function wrapper({ children }: { children: ReactNode }) {
  return (
    <ConnectionProvider>
      <SSEProvider>{children}</SSEProvider>
    </ConnectionProvider>
  );
}

function lastES(): MockEventSource {
  return MockEventSource.instances[MockEventSource.instances.length - 1];
}

describe('useSSE', () => {
  it('throws when used outside provider', () => {
    expect(() => {
      renderHook(() => useSSE());
    }).toThrow('useSSE must be used within SSEProvider');
  });

  it('starts with empty sessions', () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    expect(result.current.sessions).toEqual([]);
  });

  it('connects to SSE endpoint on mount', () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    renderHook(() => useSSE(), { wrapper });

    expect(MockEventSource.instances.length).toBeGreaterThanOrEqual(1);
  });

  it('sets connected on open', async () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    expect(result.current.connected).toBe(true);
  });

  it('hydrates sessions on connect', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });
    expect(result.current.sessions[0].name).toBe('my-api');
  });

  it('merges session events', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });

    const es = lastES();
    act(() => {
      es._fireEvent(
        'session',
        JSON.stringify({
          session_id: 'sess-1',
          session_name: 'my-api',
          status: 'ready',
          output_snippet: null,
        }),
      );
    });

    expect(result.current.sessions[0].status).toBe('ready');
  });

  it('setSessions updates sessions', () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => {
      result.current.setSessions([
        {
          id: 'x',
          name: 'test',
          status: 'active',
          command: 'p',
          description: null,
          workdir: '/repo',
          metadata: null,
          ink: null,
          intervention_reason: null,
          intervention_at: null,
          last_output_at: null,

          created_at: '2025-01-01T00:00:00Z',
        },
      ]);
    });

    expect(result.current.sessions).toHaveLength(1);
  });

  it('stores output_snippet from session events', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });

    const es = lastES();
    act(() => {
      es._fireEvent(
        'session',
        JSON.stringify({
          session_id: 'sess-1',
          session_name: 'my-api',
          status: 'idle',
          output_snippet: 'Do you trust this file? (Y/N)',
        }),
      );
    });

    expect(result.current.sessions[0].status).toBe('idle');
    expect(result.current.sessions[0].output_snippet).toBe('Do you trust this file? (Y/N)');
  });

  it('preserves output_snippet when event has no snippet', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
        status: 'idle',
        command: 'Fix',
        description: null,
        workdir: '/repo',
        metadata: null,
        ink: null,
        intervention_reason: null,
        intervention_at: null,
        last_output_at: null,
        output_snippet: 'Existing snippet',

        created_at: '2025-01-01T00:00:00Z',
      },
    ];
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });

    const es = lastES();
    act(() => {
      es._fireEvent(
        'session',
        JSON.stringify({
          session_id: 'sess-1',
          session_name: 'my-api',
          status: 'active',
          output_snippet: null,
        }),
      );
    });

    expect(result.current.sessions[0].status).toBe('active');
    expect(result.current.sessions[0].output_snippet).toBe('Existing snippet');
  });

  it('ignores malformed session events', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });

    const es = lastES();
    // Should not throw
    act(() => {
      es._fireEvent('session', 'not json');
    });

    expect(result.current.sessions).toHaveLength(1);
  });

  it('reconnects on error with backoff', () => {
    vi.useFakeTimers();
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    renderHook(() => useSSE(), { wrapper });

    const instancesBefore = MockEventSource.instances.length;
    const es = lastES();

    act(() => es.onerror?.());
    act(() => vi.advanceTimersByTime(1000));

    expect(MockEventSource.instances.length).toBeGreaterThan(instancesBefore);
  });

  it('handles hydration failure gracefully', async () => {
    mockFetch.mockRejectedValue(new Error('Network error'));
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.connected).toBe(true);
    });
    expect(result.current.sessions).toEqual([]);
  });

  it('re-fetches when session event has unknown session_id', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });

    mockFetch.mockClear();
    const updatedSessions = [
      ...sessions,
      {
        id: 'sess-2',
        name: 'new-session',
        status: 'active',
        command: 'New',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(updatedSessions) });

    act(() => {
      lastES()._fireEvent(
        'session',
        JSON.stringify({
          session_id: 'sess-unknown',
          session_name: 'unknown',
          status: 'active',
          output_snippet: null,
        }),
      );
    });

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
  });

  it('hydrates sessions eagerly before SSE opens', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'eager',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    // Sessions appear WITHOUT calling onopen
    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });
    expect(result.current.sessions[0].name).toBe('eager');
    expect(result.current.connected).toBe(false);
  });

  it('cleans up on unmount', () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { unmount } = renderHook(() => useSSE(), { wrapper });

    const es = lastES();
    unmount();

    expect(es.closed).toBe(true);
  });

  it('includes auth token in SSE URL', () => {
    testToken = 'my-secret';
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    renderHook(() => useSSE(), { wrapper });

    const es = lastES();
    expect(es.url).toContain('token=my-secret');
  });

  it('creates new EventSource after reconnect timer fires', () => {
    vi.useFakeTimers();
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    renderHook(() => useSSE(), { wrapper });

    const firstES = lastES();

    // Error handler closes the ES and schedules reconnect
    act(() => firstES.onerror?.());
    expect(firstES.closed).toBe(true);

    // Advance timer to trigger reconnect — should create a new ES
    act(() => vi.advanceTimersByTime(1000));

    const secondES = lastES();
    expect(secondES).not.toBe(firstES);
    expect(secondES.closed).toBe(false);
  });

  it('reconnects immediately on visibilitychange when SSE is dead', () => {
    vi.useFakeTimers();
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    renderHook(() => useSSE(), { wrapper });

    const es = lastES();
    // Simulate SSE dying (error closes it, backoff timer scheduled at 1s)
    act(() => es.onerror?.());
    expect(es.closed).toBe(true);

    const countBefore = MockEventSource.instances.length;

    // Page becomes visible — should reconnect immediately without waiting for backoff
    act(() => {
      Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));
    });

    // New EventSource created without advancing timers
    expect(MockEventSource.instances.length).toBeGreaterThan(countBefore);
  });

  it('hydrates on visibilitychange when SSE is still connected', async () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());
    expect(result.current.connected).toBe(true);

    mockFetch.mockClear();
    mockFetch.mockResolvedValue({
      json: () =>
        Promise.resolve([
          {
            id: 'sess-1',
            name: 'refreshed',
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
        ]),
    });

    // Page becomes visible — should re-hydrate sessions
    act(() => {
      Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));
    });

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
  });

  it('does nothing on visibilitychange when hidden', () => {
    vi.useFakeTimers();
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    renderHook(() => useSSE(), { wrapper });

    const es = lastES();
    act(() => es.onerror?.());

    const countBefore = MockEventSource.instances.length;

    act(() => {
      Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));
    });

    // Should NOT reconnect when going hidden
    expect(MockEventSource.instances.length).toBe(countBefore);
  });

  it('disconnect clears reconnect timer', () => {
    vi.useFakeTimers();
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { unmount } = renderHook(() => useSSE(), { wrapper });

    const es = lastES();
    // Trigger error to schedule reconnect timer
    act(() => es.onerror?.());

    // Unmount triggers disconnect, which should clear the timer
    unmount();

    // Advancing should NOT create a new EventSource
    const countBefore = MockEventSource.instances.length;
    act(() => vi.advanceTimersByTime(5000));
    expect(MockEventSource.instances.length).toBe(countBefore);
  });
});
