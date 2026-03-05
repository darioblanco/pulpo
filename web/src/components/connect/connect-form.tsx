import { useState, useEffect, useRef, useCallback } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { testConnection } from '@/api/connection';

type NodeStatus = 'unknown' | 'checking' | 'online' | 'offline';

interface ConnectFormProps {
  onConnect: (url: string, token: string, nodeName: string) => void;
  initialToken?: string;
}

const DEFAULT_URL = 'http://localhost:7433';

export function ConnectForm({ onConnect, initialToken = '' }: ConnectFormProps) {
  const [url, setUrl] = useState(DEFAULT_URL);
  const [token, setToken] = useState(initialToken);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nodeStatus, setNodeStatus] = useState<NodeStatus>('unknown');
  const probeTimer = useRef<ReturnType<typeof setTimeout>>(null);

  const probeHealth = useCallback(async (targetUrl: string) => {
    if (!targetUrl.trim()) {
      setNodeStatus('unknown');
      return;
    }
    setNodeStatus('checking');
    try {
      const res = await fetch(`${targetUrl.trim()}/api/v1/health`);
      setNodeStatus(res.ok ? 'online' : 'offline');
    } catch {
      setNodeStatus('offline');
    }
  }, []);

  // Probe on mount and whenever URL changes (debounced)
  useEffect(() => {
    if (probeTimer.current) clearTimeout(probeTimer.current);
    probeTimer.current = setTimeout(() => probeHealth(url), 500);
    return () => {
      if (probeTimer.current) clearTimeout(probeTimer.current);
    };
  }, [url, probeHealth]);

  async function handleConnect() {
    if (!url.trim()) return;
    setConnecting(true);
    setError(null);
    try {
      const node = await testConnection(url.trim(), token || undefined);
      onConnect(url.trim(), token, node.name);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Connection failed');
    } finally {
      setConnecting(false);
    }
  }

  const statusIndicator: Record<NodeStatus, { dot: string; label: string }> = {
    unknown: { dot: 'bg-muted', label: '' },
    checking: { dot: 'bg-yellow-500 animate-pulse', label: 'Checking...' },
    online: { dot: 'bg-green-500', label: 'Online' },
    offline: { dot: 'bg-red-500', label: 'Offline' },
  };

  const { dot, label } = statusIndicator[nodeStatus];

  return (
    <div data-testid="connect-form" className="space-y-4">
      <div>
        <div className="mb-1 flex items-center justify-between">
          <Label htmlFor="connect-url">Node URL</Label>
          {nodeStatus !== 'unknown' && (
            <span data-testid="node-status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span className={`inline-block h-2 w-2 rounded-full ${dot}`} />
              {label}
            </span>
          )}
        </div>
        <Input
          id="connect-url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="http://10.0.0.1:7433"
        />
      </div>
      <div>
        <Label htmlFor="connect-token">Auth token</Label>
        <Input
          id="connect-token"
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder="Optional"
        />
      </div>
      {error && <p className="text-sm text-destructive">{error}</p>}
      <Button
        data-testid="connect-btn"
        className="w-full"
        onClick={handleConnect}
        disabled={connecting}
      >
        {connecting ? 'Connecting...' : 'Connect'}
      </Button>
    </div>
  );
}
