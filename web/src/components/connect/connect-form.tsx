import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { testConnection } from '@/api/connection';

interface ConnectFormProps {
  onConnect: (url: string, token: string, nodeName: string) => void;
  initialToken?: string;
}

export function ConnectForm({ onConnect, initialToken = '' }: ConnectFormProps) {
  const [url, setUrl] = useState('');
  const [token, setToken] = useState(initialToken);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

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

  return (
    <div data-testid="connect-form" className="space-y-4">
      <div>
        <Label htmlFor="connect-url">Node URL</Label>
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
