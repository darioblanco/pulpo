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
        provider: 'claude',
        status: 'active',
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
        provider: 'claude',
        status: 'active',
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
          status: 'finished',
          output_snippet: null,
          waiting_for_input: null,
        }),
      );
    });

    expect(result.current.sessions[0].status).toBe('finished');
  });

  it('setSessions updates sessions', () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => {
      result.current.setSessions([
        {
          id: 'x',
          name: 'test',
          provider: 'claude',
          status: 'active',
          prompt: 'p',
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
      ]);
    });

    expect(result.current.sessions).toHaveLength(1);
  });

  it('ignores malformed session events', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
        provider: 'claude',
        status: 'active',
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
        provider: 'claude',
        status: 'active',
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
        provider: 'claude',
        status: 'active',
        prompt: 'New',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(updatedSessions) });

    act(() => {
      lastES()._fireEvent(
        'session',
        JSON.stringify({
          session_id: 'sess-unknown',
          session_name: 'unknown',
          status: 'active',
          output_snippet: null,
          waiting_for_input: null,
        }),
      );
    });

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });
  });

  it('updates waiting_for_input from event', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'my-api',
        provider: 'claude',
        status: 'active',
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
    mockFetch.mockResolvedValue({ json: () => Promise.resolve(sessions) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());

    await waitFor(() => {
      expect(result.current.sessions).toHaveLength(1);
    });

    act(() => {
      lastES()._fireEvent(
        'session',
        JSON.stringify({
          session_id: 'sess-1',
          session_name: 'my-api',
          status: 'active',
          output_snippet: null,
          waiting_for_input: true,
        }),
      );
    });

    expect(result.current.sessions[0].waiting_for_input).toBe(true);
  });

  it('hydrates sessions eagerly before SSE opens', async () => {
    const sessions = [
      {
        id: 'sess-1',
        name: 'eager',
        provider: 'claude',
        status: 'active',
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

  it('starts with cultureVersion 0', () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { result } = renderHook(() => useSSE(), { wrapper });
    expect(result.current.cultureVersion).toBe(0);
  });

  it('increments cultureVersion on culture event', async () => {
    mockFetch.mockResolvedValue({ json: () => Promise.resolve([]) });
    const { result } = renderHook(() => useSSE(), { wrapper });

    act(() => lastES().onopen?.());
    expect(result.current.cultureVersion).toBe(0);

    act(() => {
      lastES()._fireEvent(
        'culture',
        JSON.stringify({ action: 'synced', count: 1, node_name: 'n', timestamp: 't' }),
      );
    });

    expect(result.current.cultureVersion).toBe(1);
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
