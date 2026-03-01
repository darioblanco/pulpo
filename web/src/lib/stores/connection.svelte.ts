export interface SavedConnection {
  name: string;
  url: string;
  token?: string;
  lastConnected: string;
}

const ACTIVE_URL_KEY = 'pulpo:activeUrl';
const CONNECTIONS_KEY = 'pulpo:connections';
const AUTH_TOKEN_KEY = 'pulpo:authToken';

let baseUrl = $state('');
let authToken = $state('');
let savedConnections = $state<SavedConnection[]>([]);

/** Detect whether we're running inside a Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Sync current URL + token to the Tauri native bridge (`connection.json`).
 * No-op when running in a plain browser.
 */
async function syncToTauriBridge(url: string, token: string): Promise<void> {
  if (!isTauri()) return;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('save_connection', { url, token });
  } catch {
    // Best-effort — native bridge may not be available in older builds
  }
}

export function getBaseUrl(): string {
  return baseUrl;
}

export function setBaseUrl(url: string): void {
  baseUrl = url;
  if (url) {
    localStorage.setItem(ACTIVE_URL_KEY, url);
  } else {
    localStorage.removeItem(ACTIVE_URL_KEY);
  }
  void syncToTauriBridge(url, authToken);
}

export function isConnected(): boolean {
  return baseUrl !== '';
}

export function getAuthToken(): string {
  return authToken;
}

export function setAuthToken(token: string): void {
  authToken = token;
  if (token) {
    localStorage.setItem(AUTH_TOKEN_KEY, token);
  } else {
    localStorage.removeItem(AUTH_TOKEN_KEY);
  }
  void syncToTauriBridge(baseUrl, token);
}

export function disconnect(): void {
  setBaseUrl('');
  setAuthToken('');
}

export function getSavedConnections(): SavedConnection[] {
  return savedConnections;
}

export function addSavedConnection(conn: SavedConnection): void {
  const existing = savedConnections.findIndex((c) => c.url === conn.url);
  if (existing >= 0) {
    savedConnections[existing] = conn;
  } else {
    savedConnections.push(conn);
  }
  localStorage.setItem(CONNECTIONS_KEY, JSON.stringify(savedConnections));
}

export function removeSavedConnection(url: string): void {
  savedConnections = savedConnections.filter((c) => c.url !== url);
  localStorage.setItem(CONNECTIONS_KEY, JSON.stringify(savedConnections));
}

export function loadSavedConnections(): void {
  const stored = localStorage.getItem(CONNECTIONS_KEY);
  if (stored) {
    try {
      savedConnections = JSON.parse(stored);
    } catch {
      savedConnections = [];
    }
  } else {
    savedConnections = [];
  }
  const activeUrl = localStorage.getItem(ACTIVE_URL_KEY);
  baseUrl = activeUrl ?? '';
  const storedToken = localStorage.getItem(AUTH_TOKEN_KEY);
  authToken = storedToken ?? '';
}
