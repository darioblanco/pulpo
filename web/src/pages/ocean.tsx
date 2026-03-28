import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { TidePool } from '@/components/ocean/tide-pool';
import { Skeleton } from '@/components/ui/skeleton';
import { getPeers, getRemoteSessions, stopSession } from '@/api/client';
import { loadAllSprites, type Sprites } from '@/components/ocean/engine/sprites';
import { NODE_COLORS } from '@/components/ocean/engine/world';
import { useSSE } from '@/hooks/use-sse';
import type { NodeInfo, PeerInfo, Session } from '@/api/types';

interface TidePoolEntry {
  nodeName: string;
  isLocal: boolean;
  nodeStatus: 'online' | 'offline' | 'unknown';
  sessions: Session[];
  nodeColor: string;
}

export function OceanPage() {
  const { sessions } = useSSE();
  const [localNode, setLocalNode] = useState<NodeInfo | null>(null);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [peerSessions, setPeerSessions] = useState<Record<string, Session[]>>({});
  const [sprites, setSprites] = useState<Sprites | null>(null);
  const [focusedNode, setFocusedNode] = useState<string | null>(null);

  // Load shared sprites once
  useEffect(() => {
    loadAllSprites()
      .then((s) => setSprites(s))
      .catch(() => {
        /* sprites will be null — pools render gradient fallback */
      });
  }, []);

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

  // Build tide pool entries
  const pools: TidePoolEntry[] = [];
  if (localNode) {
    pools.push({
      nodeName: localNode.name,
      isLocal: true,
      nodeStatus: 'online',
      sessions,
      nodeColor: NODE_COLORS[0 % NODE_COLORS.length],
    });
    for (let i = 0; i < peers.length; i++) {
      pools.push({
        nodeName: peers[i].name,
        isLocal: false,
        nodeStatus: peers[i].status,
        sessions: peerSessions[peers[i].name] ?? [],
        nodeColor: NODE_COLORS[(i + 1) % NODE_COLORS.length],
      });
    }
  }

  const handleStop = useCallback(
    async (sessionName: string) => {
      const session = sessions.find((s) => s.name === sessionName);
      if (!session) return;
      try {
        await stopSession(session.id);
      } catch {
        /* ignore — SSE will update status */
      }
    },
    [sessions],
  );

  const handlePurge = useCallback(
    async (sessionName: string) => {
      const session = sessions.find((s) => s.name === sessionName);
      if (!session) return;
      try {
        await stopSession(session.id, true);
      } catch {
        /* ignore — SSE will update */
      }
    },
    [sessions],
  );

  // Grid columns: 1 by default, 2 at 2xl, 3 beyond 2xl
  const gridCols =
    pools.length <= 1 ? 'grid-cols-1' : 'grid-cols-1 2xl:grid-cols-2 min-[1800px]:grid-cols-3';

  const showExpand = pools.length > 1;
  const visiblePools = focusedNode ? pools.filter((p) => p.nodeName === focusedNode) : pools;

  return (
    <div data-testid="ocean-page">
      <AppHeader title="The Ocean" />
      <div className={`p-4 sm:p-6 ${focusedNode ? 'overflow-hidden' : ''}`}>
        {!localNode ? (
          <div data-testid="loading-skeleton">
            <Skeleton className="h-[400px] w-full rounded-lg" />
          </div>
        ) : (
          <>
            <div
              className={`grid ${focusedNode ? 'grid-cols-1' : gridCols} gap-4`}
              data-testid="tide-pool-grid"
            >
              {visiblePools.map((pool) => (
                <TidePool
                  key={pool.nodeName}
                  nodeName={pool.nodeName}
                  isLocal={pool.isLocal}
                  nodeStatus={pool.nodeStatus}
                  sessions={pool.sessions}
                  nodeColor={pool.nodeColor}
                  sprites={sprites}
                  expanded={focusedNode === pool.nodeName}
                  onToggleExpand={
                    showExpand
                      ? () =>
                          setFocusedNode((prev) => (prev === pool.nodeName ? null : pool.nodeName))
                      : undefined
                  }
                  onStopSession={pool.isLocal ? handleStop : undefined}
                  onPurgeSession={pool.isLocal ? handlePurge : undefined}
                />
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
