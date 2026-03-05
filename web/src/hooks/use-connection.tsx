import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react';
import { setApiConfig } from '@/api/client';

export interface SavedConnection {
  name: string;
  url: string;
  token?: string;
  lastConnected: string;
}

const ACTIVE_URL_KEY = 'pulpo:activeUrl';
const CONNECTIONS_KEY = 'pulpo:connections';
const AUTH_TOKEN_KEY = 'pulpo:authToken';

interface ConnectionContextValue {
  baseUrl: string;
  setBaseUrl: (url: string) => void;
  authToken: string;
  setAuthToken: (token: string) => void;
  isConnected: boolean;
  disconnect: () => void;
  savedConnections: SavedConnection[];
  addSavedConnection: (conn: SavedConnection) => void;
  removeSavedConnection: (url: string) => void;
}

const ConnectionContext = createContext<ConnectionContextValue | null>(null);

export function ConnectionProvider({ children }: { children: ReactNode }) {
  const [baseUrl, setBaseUrlState] = useState('');
  const [authToken, setAuthTokenState] = useState('');
  const [savedConnections, setSavedConnections] = useState<SavedConnection[]>([]);

  // Load from localStorage on mount
  useEffect(() => {
    const stored = localStorage.getItem(CONNECTIONS_KEY);
    if (stored) {
      try {
        setSavedConnections(JSON.parse(stored));
      } catch {
        setSavedConnections([]);
      }
    }
    const activeUrl = localStorage.getItem(ACTIVE_URL_KEY);
    if (activeUrl) setBaseUrlState(activeUrl);
    const storedToken = localStorage.getItem(AUTH_TOKEN_KEY);
    if (storedToken) setAuthTokenState(storedToken);
  }, []);

  // Keep API client in sync with connection state
  useEffect(() => {
    setApiConfig({
      getBaseUrl: () => baseUrl,
      getAuthToken: () => authToken,
    });
  }, [baseUrl, authToken]);

  const setBaseUrl = useCallback((url: string) => {
    setBaseUrlState(url);
    if (url) {
      localStorage.setItem(ACTIVE_URL_KEY, url);
    } else {
      localStorage.removeItem(ACTIVE_URL_KEY);
    }
  }, []);

  const setAuthToken = useCallback((token: string) => {
    setAuthTokenState(token);
    if (token) {
      localStorage.setItem(AUTH_TOKEN_KEY, token);
    } else {
      localStorage.removeItem(AUTH_TOKEN_KEY);
    }
  }, []);

  const disconnect = useCallback(() => {
    setBaseUrl('');
    setAuthToken('');
  }, [setBaseUrl, setAuthToken]);

  const addSavedConnection = useCallback((conn: SavedConnection) => {
    setSavedConnections((prev) => {
      const existing = prev.findIndex((c) => c.url === conn.url);
      let next: SavedConnection[];
      if (existing >= 0) {
        next = [...prev];
        next[existing] = conn;
      } else {
        next = [...prev, conn];
      }
      localStorage.setItem(CONNECTIONS_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  const removeSavedConnection = useCallback((url: string) => {
    setSavedConnections((prev) => {
      const next = prev.filter((c) => c.url !== url);
      localStorage.setItem(CONNECTIONS_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  return (
    <ConnectionContext.Provider
      value={{
        baseUrl,
        setBaseUrl,
        authToken,
        setAuthToken,
        isConnected: baseUrl !== '',
        disconnect,
        savedConnections,
        addSavedConnection,
        removeSavedConnection,
      }}
    >
      {children}
    </ConnectionContext.Provider>
  );
}

export function useConnection(): ConnectionContextValue {
  const ctx = useContext(ConnectionContext);
  if (!ctx) throw new Error('useConnection must be used within ConnectionProvider');
  return ctx;
}
