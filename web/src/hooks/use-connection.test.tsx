import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { ConnectionProvider, useConnection } from './use-connection';
import type { ReactNode } from 'react';

const mockLocalStorage: Record<string, string> = {};

beforeEach(() => {
  Object.keys(mockLocalStorage).forEach((key) => delete mockLocalStorage[key]);
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => mockLocalStorage[key] ?? null,
    setItem: (key: string, value: string) => {
      mockLocalStorage[key] = value;
    },
    removeItem: (key: string) => {
      delete mockLocalStorage[key];
    },
  });
});

function wrapper({ children }: { children: ReactNode }) {
  return <ConnectionProvider>{children}</ConnectionProvider>;
}

describe('useConnection', () => {
  it('throws when used outside provider', () => {
    expect(() => {
      renderHook(() => useConnection());
    }).toThrow('useConnection must be used within ConnectionProvider');
  });

  it('starts with empty state', () => {
    const { result } = renderHook(() => useConnection(), { wrapper });

    expect(result.current.baseUrl).toBe('');
    expect(result.current.authToken).toBe('');
    expect(result.current.isConnected).toBe(false);
    expect(result.current.savedConnections).toEqual([]);
  });

  it('loads saved state from localStorage', () => {
    mockLocalStorage['pulpo:activeUrl'] = 'http://mac-mini:7433';
    mockLocalStorage['pulpo:authToken'] = 'saved-token';
    mockLocalStorage['pulpo:connections'] = JSON.stringify([
      { name: 'Mac Mini', url: 'http://mac-mini:7433', lastConnected: '2026-01-01' },
    ]);

    const { result } = renderHook(() => useConnection(), { wrapper });

    expect(result.current.baseUrl).toBe('http://mac-mini:7433');
    expect(result.current.authToken).toBe('saved-token');
    expect(result.current.isConnected).toBe(true);
    expect(result.current.savedConnections).toHaveLength(1);
  });

  it('handles corrupt localStorage gracefully', () => {
    mockLocalStorage['pulpo:connections'] = 'not json';

    const { result } = renderHook(() => useConnection(), { wrapper });

    expect(result.current.savedConnections).toEqual([]);
  });

  it('setBaseUrl persists to localStorage', () => {
    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() => result.current.setBaseUrl('http://new:7433'));

    expect(result.current.baseUrl).toBe('http://new:7433');
    expect(mockLocalStorage['pulpo:activeUrl']).toBe('http://new:7433');
  });

  it('setBaseUrl clears localStorage when empty', () => {
    mockLocalStorage['pulpo:activeUrl'] = 'http://old:7433';

    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() => result.current.setBaseUrl(''));

    expect(result.current.baseUrl).toBe('');
    expect(mockLocalStorage['pulpo:activeUrl']).toBeUndefined();
  });

  it('setAuthToken persists to localStorage', () => {
    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() => result.current.setAuthToken('new-token'));

    expect(result.current.authToken).toBe('new-token');
    expect(mockLocalStorage['pulpo:authToken']).toBe('new-token');
  });

  it('setAuthToken clears localStorage when empty', () => {
    mockLocalStorage['pulpo:authToken'] = 'old-token';

    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() => result.current.setAuthToken(''));

    expect(result.current.authToken).toBe('');
    expect(mockLocalStorage['pulpo:authToken']).toBeUndefined();
  });

  it('disconnect clears both url and token', () => {
    mockLocalStorage['pulpo:activeUrl'] = 'http://mac-mini:7433';
    mockLocalStorage['pulpo:authToken'] = 'token';

    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() => result.current.disconnect());

    expect(result.current.baseUrl).toBe('');
    expect(result.current.authToken).toBe('');
    expect(result.current.isConnected).toBe(false);
  });

  it('addSavedConnection adds new connection', () => {
    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() =>
      result.current.addSavedConnection({
        name: 'Mac Mini',
        url: 'http://mac-mini:7433',
        lastConnected: '2026-01-01',
      }),
    );

    expect(result.current.savedConnections).toHaveLength(1);
    expect(result.current.savedConnections[0].name).toBe('Mac Mini');
    expect(mockLocalStorage['pulpo:connections']).toBeDefined();
  });

  it('addSavedConnection updates existing connection', () => {
    mockLocalStorage['pulpo:connections'] = JSON.stringify([
      { name: 'Old Name', url: 'http://mac-mini:7433', lastConnected: '2025-01-01' },
    ]);

    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() =>
      result.current.addSavedConnection({
        name: 'New Name',
        url: 'http://mac-mini:7433',
        lastConnected: '2026-01-01',
      }),
    );

    expect(result.current.savedConnections).toHaveLength(1);
    expect(result.current.savedConnections[0].name).toBe('New Name');
  });

  it('removeSavedConnection removes by url', () => {
    mockLocalStorage['pulpo:connections'] = JSON.stringify([
      { name: 'Mac Mini', url: 'http://mac-mini:7433', lastConnected: '2026-01-01' },
      { name: 'Win PC', url: 'http://win-pc:7433', lastConnected: '2026-01-01' },
    ]);

    const { result } = renderHook(() => useConnection(), { wrapper });

    act(() => result.current.removeSavedConnection('http://mac-mini:7433'));

    expect(result.current.savedConnections).toHaveLength(1);
    expect(result.current.savedConnections[0].name).toBe('Win PC');
  });
});
