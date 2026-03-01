import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import {
  getBaseUrl,
  setBaseUrl,
  isConnected,
  disconnect,
  getSavedConnections,
  addSavedConnection,
  removeSavedConnection,
  loadSavedConnections,
  getAuthToken,
  setAuthToken,
  isTauri,
} from './connection.svelte';

beforeEach(() => {
  localStorage.clear();
  // Ensure __TAURI_INTERNALS__ is not set between tests
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  loadSavedConnections();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('connection store', () => {
  it('starts with empty base URL', () => {
    expect(getBaseUrl()).toBe('');
    expect(isConnected()).toBe(false);
  });

  it('setBaseUrl updates URL and persists to localStorage', () => {
    setBaseUrl('http://mac-mini:7433');

    expect(getBaseUrl()).toBe('http://mac-mini:7433');
    expect(isConnected()).toBe(true);
    expect(localStorage.getItem('pulpo:activeUrl')).toBe('http://mac-mini:7433');
  });

  it('setBaseUrl with empty string removes from localStorage', () => {
    setBaseUrl('http://mac-mini:7433');
    setBaseUrl('');

    expect(getBaseUrl()).toBe('');
    expect(isConnected()).toBe(false);
    expect(localStorage.getItem('pulpo:activeUrl')).toBeNull();
  });

  it('disconnect clears base URL and token', () => {
    setBaseUrl('http://mac-mini:7433');
    setAuthToken('my-token');
    disconnect();

    expect(getBaseUrl()).toBe('');
    expect(isConnected()).toBe(false);
    expect(getAuthToken()).toBe('');
    expect(localStorage.getItem('pulpo:authToken')).toBeNull();
  });
});

describe('auth token', () => {
  it('starts with empty token', () => {
    expect(getAuthToken()).toBe('');
  });

  it('setAuthToken updates and persists', () => {
    setAuthToken('my-secret');

    expect(getAuthToken()).toBe('my-secret');
    expect(localStorage.getItem('pulpo:authToken')).toBe('my-secret');
  });

  it('setAuthToken with empty string removes from localStorage', () => {
    setAuthToken('my-secret');
    setAuthToken('');

    expect(getAuthToken()).toBe('');
    expect(localStorage.getItem('pulpo:authToken')).toBeNull();
  });

  it('loadSavedConnections restores token', () => {
    localStorage.setItem('pulpo:authToken', 'restored-token');

    loadSavedConnections();

    expect(getAuthToken()).toBe('restored-token');
  });

  it('loadSavedConnections handles no stored token', () => {
    loadSavedConnections();

    expect(getAuthToken()).toBe('');
  });
});

describe('saved connections', () => {
  it('starts with empty saved connections', () => {
    expect(getSavedConnections()).toEqual([]);
  });

  it('addSavedConnection adds and persists', () => {
    const conn = { name: 'Mac Mini', url: 'http://mac-mini:7433', lastConnected: '2026-01-01' };
    addSavedConnection(conn);

    expect(getSavedConnections()).toEqual([conn]);
    expect(JSON.parse(localStorage.getItem('pulpo:connections')!)).toEqual([conn]);
  });

  it('addSavedConnection updates existing by URL', () => {
    addSavedConnection({
      name: 'Mac Mini',
      url: 'http://mac-mini:7433',
      lastConnected: '2026-01-01',
    });
    addSavedConnection({
      name: 'Mac Mini (updated)',
      url: 'http://mac-mini:7433',
      lastConnected: '2026-02-01',
    });

    const saved = getSavedConnections();
    expect(saved).toHaveLength(1);
    expect(saved[0].name).toBe('Mac Mini (updated)');
  });

  it('removeSavedConnection removes by URL', () => {
    addSavedConnection({ name: 'A', url: 'http://a:7433', lastConnected: '2026-01-01' });
    addSavedConnection({ name: 'B', url: 'http://b:7433', lastConnected: '2026-01-01' });

    removeSavedConnection('http://a:7433');

    const saved = getSavedConnections();
    expect(saved).toHaveLength(1);
    expect(saved[0].name).toBe('B');
  });

  it('loadSavedConnections restores from localStorage', () => {
    const conns = [{ name: 'Mac', url: 'http://mac:7433', lastConnected: '2026-01-01' }];
    localStorage.setItem('pulpo:connections', JSON.stringify(conns));
    localStorage.setItem('pulpo:activeUrl', 'http://mac:7433');

    loadSavedConnections();

    expect(getSavedConnections()).toEqual(conns);
    expect(getBaseUrl()).toBe('http://mac:7433');
    expect(isConnected()).toBe(true);
  });

  it('loadSavedConnections handles invalid JSON gracefully', () => {
    localStorage.setItem('pulpo:connections', 'not-json');

    loadSavedConnections();

    expect(getSavedConnections()).toEqual([]);
  });

  it('loadSavedConnections handles no stored data', () => {
    loadSavedConnections();

    expect(getSavedConnections()).toEqual([]);
    expect(getBaseUrl()).toBe('');
  });
});

describe('Tauri bridge', () => {
  it('isTauri returns false in plain browser', () => {
    expect(isTauri()).toBe(false);
  });

  it('isTauri returns true when __TAURI_INTERNALS__ is present', () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(isTauri()).toBe(true);
  });

  it('setBaseUrl calls invoke via Tauri bridge when Tauri is present', async () => {
    const tauriMock = await import('@tauri-apps/api/core');
    const invokeSpy = vi.spyOn(tauriMock, 'invoke');

    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    setBaseUrl('http://mac-mini:7433');

    // Allow the async dynamic import to resolve
    await new Promise((r) => setTimeout(r, 10));

    expect(invokeSpy).toHaveBeenCalledWith('save_connection', {
      url: 'http://mac-mini:7433',
      token: '',
    });
  });

  it('setAuthToken calls invoke via Tauri bridge when Tauri is present', async () => {
    const tauriMock = await import('@tauri-apps/api/core');
    const invokeSpy = vi.spyOn(tauriMock, 'invoke');

    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    setBaseUrl('http://mac-mini:7433');
    await new Promise((r) => setTimeout(r, 10));
    invokeSpy.mockClear();

    setAuthToken('my-token');
    await new Promise((r) => setTimeout(r, 10));

    expect(invokeSpy).toHaveBeenCalledWith('save_connection', {
      url: 'http://mac-mini:7433',
      token: 'my-token',
    });
  });

  it('syncToTauriBridge is no-op without Tauri', () => {
    // No __TAURI_INTERNALS__ → should not attempt import
    setBaseUrl('http://mac-mini:7433');
    // No error — just localStorage
    expect(localStorage.getItem('pulpo:activeUrl')).toBe('http://mac-mini:7433');
  });

  it('syncToTauriBridge swallows invoke errors gracefully', async () => {
    const tauriMock = await import('@tauri-apps/api/core');
    vi.spyOn(tauriMock, 'invoke').mockRejectedValue(new Error('tauri error'));

    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    // Should not throw
    setBaseUrl('http://mac-mini:7433');
    await new Promise((r) => setTimeout(r, 10));

    // localStorage still works
    expect(localStorage.getItem('pulpo:activeUrl')).toBe('http://mac-mini:7433');
  });
});
