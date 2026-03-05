import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router';
import { AppHeader } from '@/components/layout/app-header';
import { StatusSummary } from '@/components/dashboard/status-summary';
import { NodeCard } from '@/components/dashboard/node-card';
import { NewSessionDialog } from '@/components/dashboard/new-session-dialog';
import { Skeleton } from '@/components/ui/skeleton';
import { getPeers, getRemoteSessions } from '@/api/client';
import { useSSE } from '@/hooks/use-sse';
import { useConnection } from '@/hooks/use-connection';
import { detectStatusChanges, showDesktopNotification } from '@/lib/notifications';
import { toast } from 'sonner';
import type { NodeInfo, PeerInfo, Session } from '@/api/types';

export function DashboardPage() {
  const navigate = useNavigate();
  const { isConnected } = useConnection();
  const { sessions, setSessions, connected } = useSSE();
  const [localNode, setLocalNode] = useState<NodeInfo | null>(null);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [peerSessions, setPeerSessions] = useState<Record<string, Session[]>>({});
  const [error, setError] = useState<string | null>(null);
  const previousSessionsRef = useRef<Session[]>([]);

  const fetchPeers = useCallback(async () => {
    try {
      const resp = await getPeers();
      setLocalNode(resp.local);
      setPeers(resp.peers);

      const peerResults: Record<string, Session[]> = {};
      const promises = resp.peers
        .filter((p) => p.status === 'online')
        .map(async (peer) => {
          try {
            peerResults[peer.name] = await getRemoteSessions(peer.address);
          } catch {
            peerResults[peer.name] = [];
          }
        });
      await Promise.all(promises);
      setPeerSessions(peerResults);
      setError(null);
    } catch {
      if (!isConnected) {
        navigate('/connect');
        return;
      }
      setError('Failed to connect to pulpod');
    }
  }, [isConnected, navigate]);

  // Fetch peers on mount and poll
  useEffect(() => {
    fetchPeers();
    const interval = setInterval(fetchPeers, 30000);
    return () => clearInterval(interval);
  }, [fetchPeers]);

  const handleSessionCreated = useCallback(
    (session: Session) => {
      setSessions((prev: Session[]) => [...prev, session]);
      fetchPeers();
    },
    [setSessions, fetchPeers],
  );

  // Notification processing when SSE sessions change
  useEffect(() => {
    if (previousSessionsRef.current.length > 0) {
      const changes = detectStatusChanges(previousSessionsRef.current, sessions);
      for (const change of changes) {
        const label =
          change.to === 'completed' ? 'completed' : change.to === 'dead' ? 'died' : 'resumed';
        toast(`${change.sessionName} ${label}`);
        showDesktopNotification(change);
      }
    }
    previousSessionsRef.current = sessions;
  }, [sessions]);

  const activeSessions = sessions.filter(
    (s) => s.status === 'creating' || s.status === 'running' || s.status === 'stale',
  );

  return (
    <div data-testid="dashboard-page">
      <AppHeader title="Dashboard" />
      <div className="space-y-6 p-4 sm:p-6">
        {error ? (
          <p className="text-center text-destructive">{error}</p>
        ) : !connected && !localNode ? (
          <div className="space-y-4" data-testid="loading-skeleton">
            <Skeleton className="h-20 w-full" />
            <Skeleton className="h-40 w-full" />
          </div>
        ) : (
          <>
            <div className="flex flex-wrap items-center justify-between gap-3">
              <StatusSummary sessions={sessions} />
              <NewSessionDialog peers={peers} onCreated={handleSessionCreated} />
            </div>

            {localNode && (
              <NodeCard
                name={localNode.name}
                nodeInfo={localNode}
                status="online"
                sessions={activeSessions}
                isLocal
                onRefresh={fetchPeers}
              />
            )}

            {peers.map((peer) => (
              <NodeCard
                key={peer.name}
                name={peer.name}
                nodeInfo={peer.node_info}
                status={peer.status}
                sessions={peerSessions[peer.name] ?? []}
                onRefresh={fetchPeers}
              />
            ))}
          </>
        )}
      </div>
    </div>
  );
}
