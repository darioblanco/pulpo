import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router';
import { AppHeader } from '@/components/layout/app-header';
import { StatusSummary } from '@/components/dashboard/status-summary';
import { NodeCard } from '@/components/dashboard/node-card';
import { NewSessionDialog } from '@/components/dashboard/new-session-dialog';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { getPeers, getRemoteSessions, getFleetSessions } from '@/api/client';
import { useSSE } from '@/hooks/use-sse';
import { useConnection } from '@/hooks/use-connection';
import { detectStatusChanges, showDesktopNotification } from '@/lib/notifications';
import { formatMemory } from '@/lib/utils';
import { toast } from 'sonner';
import type { NodeInfo, PeerInfo, Session, FleetSession } from '@/api/types';

export function DashboardPage() {
  const navigate = useNavigate();
  const { isConnected } = useConnection();
  const { sessions, setSessions, connected } = useSSE();
  const [localNode, setLocalNode] = useState<NodeInfo | null>(null);
  const [peers, setPeers] = useState<PeerInfo[]>([]);
  const [peerSessions, setPeerSessions] = useState<Record<string, Session[]>>({});
  const [fleetSessions, setFleetSessions] = useState<FleetSession[]>([]);
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
      // Fetch fleet sessions in parallel with peer sessions
      const fleetPromise = getFleetSessions()
        .then((r) => setFleetSessions(r.sessions))
        .catch(() => setFleetSessions([]));
      await Promise.all([...promises, fleetPromise]);
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
          change.to === 'ready' ? 'ready' : change.to === 'killed' ? 'killed' : 'resumed';
        toast(`${change.sessionName} ${label}`);
        showDesktopNotification(change);
      }
    }
    previousSessionsRef.current = sessions;
  }, [sessions]);

  const activeSessions = sessions.filter(
    (s) =>
      s.status === 'creating' ||
      s.status === 'active' ||
      s.status === 'idle' ||
      s.status === 'lost',
  );

  const hasMultipleNodes = peers.length > 0;

  return (
    <div data-testid="dashboard-page">
      <AppHeader title="Sessions" />
      <div className="space-y-4 p-4 sm:p-6">
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

            {hasMultipleNodes ? (
              <Tabs defaultValue="all" data-testid="node-tabs">
                <TabsList>
                  <TabsTrigger value="all" data-testid="tab-all">
                    <div className="flex items-center">
                      <span className="mr-1.5 inline-block h-2 w-2 rounded-full bg-primary" />
                      All
                      <span className="ml-1.5 text-xs text-muted-foreground">
                        (
                        {
                          fleetSessions.filter((s) =>
                            ['creating', 'active', 'idle', 'lost'].includes(s.status),
                          ).length
                        }
                        )
                      </span>
                    </div>
                  </TabsTrigger>
                  <TabsTrigger value="local" data-testid="tab-local">
                    <div className="flex flex-col items-start leading-tight">
                      <div className="flex items-center">
                        <span className="mr-1.5 inline-block h-2 w-2 rounded-full bg-status-ready" />
                        {localNode?.name ?? 'local'}
                        <span className="ml-1.5 text-xs text-muted-foreground">
                          ({activeSessions.length})
                        </span>
                      </div>
                      {localNode && (
                        <span
                          className="ml-3.5 text-[0.625rem] text-muted-foreground"
                          data-testid="tab-local-subtitle"
                        >
                          {localNode.os} · {localNode.cpus} CPU ·{' '}
                          {formatMemory(localNode.memory_mb)}
                        </span>
                      )}
                    </div>
                  </TabsTrigger>
                  {peers.map((peer) => (
                    <TabsTrigger key={peer.name} value={peer.name} data-testid={`tab-${peer.name}`}>
                      <div className="flex flex-col items-start leading-tight">
                        <div className="flex items-center">
                          <span
                            className={`mr-1.5 inline-block h-2 w-2 rounded-full ${
                              peer.status === 'online'
                                ? 'bg-status-ready'
                                : peer.status === 'offline'
                                  ? 'bg-status-killed'
                                  : 'bg-muted-foreground'
                            }`}
                          />
                          {peer.name}
                          <span className="ml-1.5 text-xs text-muted-foreground">
                            ({(peerSessions[peer.name] ?? []).length})
                          </span>
                        </div>
                        {peer.node_info && (
                          <span
                            className="ml-3.5 text-[0.625rem] text-muted-foreground"
                            data-testid={`tab-${peer.name}-subtitle`}
                          >
                            {peer.node_info.os} · {peer.node_info.cpus} CPU ·{' '}
                            {formatMemory(peer.node_info.memory_mb)}
                          </span>
                        )}
                      </div>
                    </TabsTrigger>
                  ))}
                </TabsList>

                <TabsContent value="all">
                  <div className="space-y-1">
                    {fleetSessions.filter((s) =>
                      ['creating', 'active', 'idle', 'lost'].includes(s.status),
                    ).length === 0 ? (
                      <p className="py-8 text-center text-muted-foreground">
                        No active sessions across the fleet.
                      </p>
                    ) : (
                      <div className="rounded-lg border">
                        <table className="w-full text-sm" data-testid="fleet-table">
                          <thead>
                            <tr className="border-b text-left text-muted-foreground">
                              <th className="px-3 py-2 font-medium">Node</th>
                              <th className="px-3 py-2 font-medium">Session</th>
                              <th className="px-3 py-2 font-medium">Status</th>
                              <th className="hidden px-3 py-2 font-medium sm:table-cell">
                                Command
                              </th>
                            </tr>
                          </thead>
                          <tbody>
                            {fleetSessions
                              .filter((s) =>
                                ['creating', 'active', 'idle', 'lost'].includes(s.status),
                              )
                              .map((s) => (
                                <tr
                                  key={`${s.node_name}-${s.id}`}
                                  className="border-b last:border-0"
                                >
                                  <td className="px-3 py-2 text-muted-foreground">{s.node_name}</td>
                                  <td className="px-3 py-2 font-medium">{s.name}</td>
                                  <td className="px-3 py-2">
                                    <span
                                      className={`inline-flex items-center gap-1.5 text-xs ${
                                        s.status === 'active'
                                          ? 'text-status-active'
                                          : s.status === 'idle'
                                            ? 'text-status-idle'
                                            : s.status === 'lost'
                                              ? 'text-status-killed'
                                              : 'text-muted-foreground'
                                      }`}
                                    >
                                      <span
                                        className={`h-1.5 w-1.5 rounded-full ${
                                          s.status === 'active'
                                            ? 'bg-status-active'
                                            : s.status === 'idle'
                                              ? 'bg-status-idle'
                                              : s.status === 'lost'
                                                ? 'bg-status-killed'
                                                : 'bg-muted-foreground'
                                        }`}
                                      />
                                      {s.status}
                                    </span>
                                  </td>
                                  <td className="hidden max-w-xs truncate px-3 py-2 text-muted-foreground sm:table-cell">
                                    {s.command}
                                  </td>
                                </tr>
                              ))}
                          </tbody>
                        </table>
                      </div>
                    )}
                  </div>
                </TabsContent>

                <TabsContent value="local">
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
                </TabsContent>

                {peers.map((peer) => (
                  <TabsContent key={peer.name} value={peer.name}>
                    <NodeCard
                      name={peer.name}
                      nodeInfo={peer.node_info}
                      status={peer.status}
                      sessions={peerSessions[peer.name] ?? []}
                      address={peer.address}
                      onRefresh={fetchPeers}
                    />
                  </TabsContent>
                ))}
              </Tabs>
            ) : (
              localNode && (
                <NodeCard
                  name={localNode.name}
                  nodeInfo={localNode}
                  status="online"
                  sessions={activeSessions}
                  isLocal
                  onRefresh={fetchPeers}
                />
              )
            )}
          </>
        )}
      </div>
    </div>
  );
}
