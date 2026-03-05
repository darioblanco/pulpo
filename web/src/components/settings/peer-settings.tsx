import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { addPeer, removePeer } from '@/api/client';
import type { PeerInfo } from '@/api/types';

const statusDotColors: Record<string, string> = {
  online: 'bg-status-completed',
  offline: 'bg-status-dead',
  unknown: 'bg-muted-foreground',
};

interface PeerSettingsProps {
  peers: PeerInfo[];
  onUpdate: (peers: PeerInfo[]) => void;
}

export function PeerSettings({ peers, onUpdate }: PeerSettingsProps) {
  const [newName, setNewName] = useState('');
  const [newAddress, setNewAddress] = useState('');
  const [error, setError] = useState<string | null>(null);

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

  return (
    <div data-testid="peer-settings" className="space-y-4">
      <h3 className="text-sm font-semibold">Peers</h3>

      {peers.length === 0 ? (
        <p className="text-sm text-muted-foreground">No peers configured.</p>
      ) : (
        <div className="space-y-2">
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
                <span className="font-medium">{peer.name}</span>
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
          <Label htmlFor="new-peer-name">Peer name</Label>
          <Input
            id="new-peer-name"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="remote-node"
          />
        </div>
        <div className="flex-1">
          <Label htmlFor="new-peer-address">Peer address</Label>
          <Input
            id="new-peer-address"
            value={newAddress}
            onChange={(e) => setNewAddress(e.target.value)}
            placeholder="10.0.0.1:7433"
          />
        </div>
        <Button data-testid="add-peer-btn" size="sm" onClick={handleAdd}>
          Add
        </Button>
      </div>
    </div>
  );
}
