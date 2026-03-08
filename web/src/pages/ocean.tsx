import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { OceanCanvas } from '@/components/ocean/ocean-canvas';
import { Skeleton } from '@/components/ui/skeleton';
import { getPeers, getRemoteSessions } from '@/api/client';
import { useSSE } from '@/hooks/use-sse';
import type { NodeInfo, PeerInfo, Session } from '@/api/types';

export function OceanPage() {
  const { sessions } = useSSE();
  const [localNode, setLocalNode] = useState<NodeInfo | null>(null);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [peerSessions, setPeerSessions] = useState<Record<string, Session[]>>({});

  const fetchPeers = useCallback(async () => {
    try {
      const resp = await getPeers();
      setLocalNode(resp.local);
      setPeers(resp.peers);

      const results: Record<string, Session[]> = {};
      const promises = resp.peers
        .filter((p) => p.status === 'online')
        .map(async (peer) => {
          try {
            results[peer.name] = await getRemoteSessions(peer.address);
          } catch {
            results[peer.name] = [];
          }
        });
      await Promise.all(promises);
      setPeerSessions(results);
    } catch {
      // Silently ignore — will retry on next poll
    }
  }, []);

  useEffect(() => {
    fetchPeers();
    const interval = setInterval(fetchPeers, 15000);
    return () => clearInterval(interval);
  }, [fetchPeers]);

  return (
    <div data-testid="ocean-page">
      <AppHeader title="The Ocean" />
      <div className="p-4 sm:p-6">
        {!localNode ? (
          <div data-testid="loading-skeleton">
            <Skeleton className="h-[400px] w-full rounded-lg" />
          </div>
        ) : (
          <OceanCanvas
            localNode={localNode}
            localSessions={sessions}
            peers={peers}
            peerSessions={peerSessions}
          />
        )}
      </div>
    </div>
  );
}
