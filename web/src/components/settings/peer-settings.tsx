import { useState } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { addPeer, removePeer } from '@/api/client';
import { FormField } from './form-field';
import type { PeerInfo } from '@/api/types';

const statusDotColors: Record<string, string> = {
  online: 'bg-status-ready',
  offline: 'bg-status-stopped',
  unknown: 'bg-muted-foreground',
};

interface PeerSettingsProps {
  peers: PeerInfo[];
  onUpdate: (peers: PeerInfo[]) => void;
  bind: string;
}

export function PeerSettings({ peers, onUpdate, bind }: PeerSettingsProps) {
  const [newName, setNewName] = useState('');
  const [newAddress, setNewAddress] = useState('');
  const [error, setError] = useState<string | null>(null);

  const isLocal = bind === 'local';
  const isTailscale = bind === 'tailscale';

  async function handleAdd() {
    if (!newName.trim() || !newAddress.trim()) return;
    try {
      const resp = await addPeer(newName.trim(), newAddress.trim());
      onUpdate(resp.peers);
      setNewName('');
      setNewAddress('');
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to add peer');
    }
  }

  async function handleRemove(name: string) {
    await removePeer(name);
    onUpdate(peers.filter((p) => p.name !== name));
  }

  const description = isLocal
    ? 'Peer discovery is disabled in local mode. Switch to tailscale, public, or container to connect nodes.'
    : isTailscale
      ? 'Peers are auto-discovered via the Tailscale API. You can also add manual entries.'
      : 'Manually add peers for multi-node connectivity.';

  return (
    <Card data-testid="peer-settings">
      <CardHeader>
        <CardTitle>Peers</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        {isLocal ? (
          <p className="text-sm text-muted-foreground" data-testid="peers-disabled">
            Change bind mode to enable networking.
          </p>
        ) : (
          <>
            {peers.length === 0 ? (
              <p className="text-sm text-muted-foreground">No peers configured.</p>
            ) : (
              <div className="grid gap-2">
                {peers.map((peer) => (
                  <div
                    key={peer.name}
                    data-testid={`peer-${peer.name}`}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border px-3 py-2"
                  >
                    <div className="flex min-w-0 items-center gap-2">
                      <span
                        className={`h-2 w-2 shrink-0 rounded-full ${statusDotColors[peer.status] ?? 'bg-muted-foreground'}`}
                      />
                      <span className="text-sm font-medium">{peer.name}</span>
                      <span className="truncate text-xs text-muted-foreground">{peer.address}</span>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      data-testid={`remove-peer-${peer.name}`}
                      onClick={() => handleRemove(peer.name)}
                    >
                      Remove
                    </Button>
                  </div>
                ))}
              </div>
            )}

            {error && <p className="text-sm text-destructive">{error}</p>}

            <div className="flex items-end gap-2">
              <div className="flex-1">
                <FormField label="Peer name" htmlFor="new-peer-name">
                  <Input
                    id="new-peer-name"
                    value={newName}
                    onChange={(e) => setNewName(e.target.value)}
                    placeholder="remote-node"
                  />
                </FormField>
              </div>
              <div className="flex-1">
                <FormField label="Peer address" htmlFor="new-peer-address">
                  <Input
                    id="new-peer-address"
                    value={newAddress}
                    onChange={(e) => setNewAddress(e.target.value)}
                    placeholder="10.0.0.1:7433"
                  />
                </FormField>
              </div>
              <Button data-testid="add-peer-btn" size="sm" onClick={handleAdd}>
                Add
              </Button>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
