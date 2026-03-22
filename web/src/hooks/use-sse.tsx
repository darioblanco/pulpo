import {
  createContext,
  useContext,
  useState,
  useRef,
  useCallback,
  useEffect,
  type ReactNode,
  type Dispatch,
  type SetStateAction,
} from 'react';
import { getSessions, resolveBaseUrl, authHeaders } from '@/api/client';
import type { Session } from '@/api/types';
import { useConnection } from './use-connection';

const MAX_RECONNECT_DELAY = 30000;

interface SSEContextValue {
  connected: boolean;
  sessions: Session[];
  setSessions: Dispatch<SetStateAction<Session[]>>;
}

const SSEContext = createContext<SSEContextValue | null>(null);

function buildEventUrl(): string {
  const base = resolveBaseUrl();
  const headers = authHeaders();
  const url = `${base}/events`;
  const token = headers['Authorization']?.replace('Bearer ', '');
  if (token) {
    return `${url}?token=${encodeURIComponent(token)}`;
  }
  return url;
}

export function SSEProvider({ children }: { children: ReactNode }) {
  const { baseUrl, authToken } = useConnection();
  const [connected, setConnected] = useState(false);
  const [sessions, setSessions] = useState<Session[]>([]);
  const eventSourceRef = useRef<EventSource | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectDelayRef = useRef(1000);
  const sessionsRef = useRef<Session[]>([]);

  // Keep sessionsRef in sync
  sessionsRef.current = sessions;

  const hydrate = useCallback(async () => {
    try {
      const all = await getSessions();
      setSessions(all);
    } catch {
      // Silently ignore — will retry on next reconnect
    }
  }, []);

  const mergeSessionEvent = useCallback(
    (event: {
      session_id: string;
      session_name: string;
      status: string;
      output_snippet: string | null;
    }): boolean => {
      const current = sessionsRef.current;
      const idx = current.findIndex((s) => s.id === event.session_id);
      if (idx === -1) return false;

      setSessions(
        current.map((s, i) => {
          if (i !== idx) return s;
          return {
            ...s,
            status: event.status,
            output_snippet: event.output_snippet ?? s.output_snippet,
          };
        }),
      );
      return true;
    },
    [],
  );

  const disconnect = useCallback(() => {
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    setConnected(false);
    reconnectDelayRef.current = 1000;
  }, []);

  const connect = useCallback(() => {
    const url = buildEventUrl();
    const es = new EventSource(url);
    eventSourceRef.current = es;

    es.onopen = () => {
      setConnected(true);
      reconnectDelayRef.current = 1000;
      hydrate();
    };

    es.addEventListener('session', (e: MessageEvent) => {
      try {
        const data = JSON.parse(e.data);
        const found = mergeSessionEvent(data);
        if (!found) {
          hydrate();
        }
      } catch {
        // Ignore malformed events
      }
    });

    es.onerror = () => {
      setConnected(false);
      es.close();
      eventSourceRef.current = null;

      if (!reconnectTimerRef.current) {
        reconnectTimerRef.current = setTimeout(() => {
          reconnectTimerRef.current = null;
          connect();
        }, reconnectDelayRef.current);
        reconnectDelayRef.current = Math.min(reconnectDelayRef.current * 2, MAX_RECONNECT_DELAY);
      }
    };
  }, [hydrate, mergeSessionEvent]);

  // Hydrate sessions eagerly (don't wait for SSE to open)
  useEffect(() => {
    hydrate();
  }, [baseUrl, authToken, hydrate]);

  // Connect when baseUrl/authToken changes
  useEffect(() => {
    connect();
    return () => disconnect();
  }, [baseUrl, authToken, connect, disconnect]);

  // Reconnect immediately when the page becomes visible (e.g. mobile PWA
  // returning from background). Without this, the user waits for the
  // exponential backoff timer (up to 30 s) before SSE reconnects.
  useEffect(() => {
    function handleVisibility() {
      if (document.visibilityState !== 'visible') return;
      // If SSE is already alive, just re-hydrate in case we missed events
      if (eventSourceRef.current && !connected) {
        // SSE object exists but never opened — let the backoff handle it
        return;
      }
      if (eventSourceRef.current) {
        // Connection looks alive — refresh session data in case we missed events
        hydrate();
        return;
      }
      // SSE is dead — cancel any pending backoff and reconnect now
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      reconnectDelayRef.current = 1000;
      connect();
    }
    document.addEventListener('visibilitychange', handleVisibility);
    return () => document.removeEventListener('visibilitychange', handleVisibility);
  }, [connect, connected, hydrate]);

  return (
    <SSEContext.Provider value={{ connected, sessions, setSessions }}>
      {children}
    </SSEContext.Provider>
  );
}

export function useSSE(): SSEContextValue {
  const ctx = useContext(SSEContext);
  if (!ctx) throw new Error('useSSE must be used within SSEProvider');
  return ctx;
}
