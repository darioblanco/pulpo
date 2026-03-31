import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useNavigate } from 'react-router';
import { AppHeader } from '@/components/layout/app-header';
import { StatusSummary } from '@/components/dashboard/status-summary';
import { NodeCard } from '@/components/dashboard/node-card';
import { NewSessionDialog } from '@/components/dashboard/new-session-dialog';
import { SessionFilter } from '@/components/history/session-filter';
import { Skeleton } from '@/components/ui/skeleton';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import {
  getPeers,
  getRemoteSessions,
  getFleetSessions,
  getSessions,
  cleanupSessions,
  stopSession,
} from '@/api/client';
import { Button } from '@/components/ui/button';
import { Trash2, CheckSquare } from 'lucide-react';
import { PeerStatusDot } from '@/components/shared/peer-status-dot';
import { useSSE } from '@/hooks/use-sse';
import { useConnection } from '@/hooks/use-connection';
import { detectStatusChanges, showDesktopNotification } from '@/lib/notifications';
import { formatMemory, statusColors } from '@/lib/utils';
import { toast } from 'sonner';
import type { NodeInfo, PeerInfo, Session, FleetSession } from '@/api/types';

const DEFAULT_STATUSES = new Set(['active', 'idle', 'ready']);

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
  const [selectedStatuses, setSelectedStatuses] = useState<Set<string>>(DEFAULT_STATUSES);
  const [searchQuery, setSearchQuery] = useState<string | undefined>(undefined);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [batchLoading, setBatchLoading] = useState(false);

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

  const handleRefresh = useCallback(async () => {
    try {
      const all = await getSessions();
      setSessions(all);
    } catch {
      // Silently ignore — SSE will re-hydrate
    }
    fetchPeers();
  }, [setSessions, fetchPeers]);

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
          change.to === 'ready' ? 'ready' : change.to === 'stopped' ? 'stopped' : 'resumed';
        toast(`${change.sessionName} ${label}`);
        showDesktopNotification(change);
      }
    }
    previousSessionsRef.current = sessions;
  }, [sessions]);

  const handleFilter = useCallback((query: { search?: string; statuses: Set<string> }) => {
    setSelectedStatuses(query.statuses);
    setSearchQuery(query.search);
  }, []);

  // Build the set of visible statuses: always include 'creating' plus selected
  const visibleStatuses = useMemo(() => {
    const s = new Set(selectedStatuses);
    s.add('creating');
    return s;
  }, [selectedStatuses]);

  const filteredSessions = useMemo(() => {
    let result = sessions.filter((s) => visibleStatuses.has(s.status));
    if (searchQuery) {
      const q = searchQuery.toLowerCase();
      result = result.filter(
        (s) => s.name.toLowerCase().includes(q) || s.command.toLowerCase().includes(q),
      );
    }
    return result;
  }, [sessions, visibleStatuses, searchQuery]);

  const hasCleanable = useMemo(
    () => sessions.some((s) => s.status === 'stopped' || s.status === 'lost'),
    [sessions],
  );

  // Prune stale selections when sessions change
  useEffect(() => {
    if (selectedIds.size === 0) return;
    const currentIds = new Set(sessions.map((s) => s.id));
    const pruned = new Set([...selectedIds].filter((id) => currentIds.has(id)));
    if (pruned.size !== selectedIds.size) {
      setSelectedIds(pruned);
    }
  }, [sessions, selectedIds]);

  const handleCleanup = useCallback(async () => {
    try {
      const result = await cleanupSessions();
      toast(`Cleaned up ${result.deleted} session${result.deleted !== 1 ? 's' : ''}`);
      handleRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to cleanup sessions');
    }
  }, [handleRefresh]);

  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const toggleSelectAll = useCallback(() => {
    const visibleIds = filteredSessions.map((s) => s.id);
    const allSelected = visibleIds.every((id) => selectedIds.has(id));
    if (allSelected) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(visibleIds));
    }
  }, [filteredSessions, selectedIds]);

  const handleBatchAction = useCallback(
    async (purge: boolean) => {
      const verb = purge ? 'Deleted' : 'Stopped';
      const count = selectedIds.size;
      setBatchLoading(true);
      try {
        const results = await Promise.allSettled(
          [...selectedIds].map((id) => stopSession(id, purge || undefined)),
        );
        const failed = results.filter((r) => r.status === 'rejected').length;
        if (failed === 0) {
          toast(`${verb} ${count} session${count !== 1 ? 's' : ''}`);
        } else {
          toast.error(`${verb} ${count - failed}/${count} sessions (${failed} failed)`);
        }
        setSelectedIds(new Set());
        setSelectionMode(false);
        handleRefresh();
      } catch (e) {
        toast.error(e instanceof Error ? e.message : `Failed to ${verb.toLowerCase()} sessions`);
      } finally {
        setBatchLoading(false);
      }
    },
    [selectedIds, handleRefresh],
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
              <div className="flex items-center gap-2">
                <Button
                  variant={selectionMode ? 'default' : 'outline'}
                  data-testid="select-mode-button"
                  onClick={() => {
                    setSelectionMode((prev) => !prev);
                    if (selectionMode) setSelectedIds(new Set());
                  }}
                >
                  <CheckSquare className="mr-2 h-4 w-4" />
                  Select
                </Button>
                {hasCleanable && (
                  <Button
                    variant="outline"
                    data-testid="cleanup-button"
                    className="text-destructive"
                    onClick={handleCleanup}
                  >
                    <Trash2 className="mr-2 h-4 w-4" />
                    Cleanup
                  </Button>
                )}
                <NewSessionDialog peers={peers} onCreated={handleSessionCreated} />
              </div>
            </div>

            <div className="flex items-center gap-3">
              <div className="flex-1">
                <SessionFilter onFilter={handleFilter} />
              </div>
              {selectionMode && (
                <label
                  data-testid="select-all-checkbox"
                  className="flex shrink-0 cursor-pointer items-center gap-2 text-sm text-muted-foreground"
                >
                  <input
                    type="checkbox"
                    checked={
                      filteredSessions.length > 0 &&
                      filteredSessions.every((s) => selectedIds.has(s.id))
                    }
                    onChange={toggleSelectAll}
                    className="h-4 w-4 rounded accent-primary"
                  />
                  All
                </label>
              )}
            </div>

            {hasMultipleNodes ? (
              <Tabs defaultValue="all" data-testid="node-tabs">
                <TabsList className="h-auto min-h-12 w-auto max-w-full justify-start overflow-x-auto py-1.5">
                  <TabsTrigger value="all" data-testid="tab-all">
                    <div className="flex items-center">
                      <span className="mr-1.5 inline-block h-2 w-2 rounded-full bg-primary" />
                      All
                      <span className="ml-1.5 text-xs text-muted-foreground">
                        ({fleetSessions.filter((s) => visibleStatuses.has(s.status)).length})
                      </span>
                    </div>
                  </TabsTrigger>
                  <TabsTrigger value="local" data-testid="tab-local">
                    <div className="flex flex-col items-start leading-tight">
                      <div className="flex items-center">
                        <span className="mr-1.5 inline-block h-2 w-2 rounded-full bg-status-ready" />
                        {localNode?.name ?? 'local'}
                        <span className="ml-1.5 text-xs text-muted-foreground">
                          ({filteredSessions.length})
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
                          <span className="mr-1.5">
                            <PeerStatusDot
                              name={peer.name}
                              address={peer.address}
                              status={peer.status}
                              testId={`peer-dot-${peer.name}`}
                            />
                          </span>
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
                    {fleetSessions.filter((s) => visibleStatuses.has(s.status)).length === 0 ? (
                      <p className="py-8 text-center text-muted-foreground">
                        No matching sessions across the fleet.
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
                              .filter((s) => visibleStatuses.has(s.status))
                              .map((s) => (
                                <tr
                                  key={`${s.node_name}-${s.id}`}
                                  className="cursor-pointer border-b last:border-0 hover:bg-muted/30"
                                  onClick={() => navigate(`/sessions/${s.id}`)}
                                >
                                  <td className="px-3 py-2 text-muted-foreground">{s.node_name}</td>
                                  <td className="px-3 py-2 font-medium">{s.name}</td>
                                  <td className="px-3 py-2">
                                    <span className="inline-flex items-center gap-1.5 text-xs">
                                      <span
                                        className={`h-1.5 w-1.5 rounded-full ${statusColors[s.status] ?? 'bg-muted-foreground'}`}
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
                      sessions={filteredSessions}
                      isLocal
                      onRefresh={handleRefresh}
                      selectionMode={selectionMode}
                      selectedIds={selectedIds}
                      onToggleSelect={toggleSelect}
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
                      onRefresh={handleRefresh}
                      selectionMode={selectionMode}
                      selectedIds={selectedIds}
                      onToggleSelect={toggleSelect}
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
                  sessions={filteredSessions}
                  isLocal
                  onRefresh={handleRefresh}
                  selectionMode={selectionMode}
                  selectedIds={selectedIds}
                  onToggleSelect={toggleSelect}
                />
              )
            )}
          </>
        )}
      </div>

      {selectionMode && selectedIds.size > 0 && (
        <div
          data-testid="batch-action-bar"
          className="fixed inset-x-0 bottom-0 z-50 flex items-center justify-center gap-3 border-t border-border bg-background/95 px-4 py-3 backdrop-blur supports-[backdrop-filter]:bg-background/60"
        >
          <span className="text-sm text-muted-foreground">{selectedIds.size} selected</span>
          <Button
            variant="outline"
            size="sm"
            data-testid="batch-stop-button"
            disabled={batchLoading}
            onClick={() => handleBatchAction(false)}
          >
            Stop
          </Button>
          <Button
            variant="destructive"
            size="sm"
            data-testid="batch-delete-button"
            disabled={batchLoading}
            onClick={() => handleBatchAction(true)}
          >
            Delete
          </Button>
        </div>
      )}
    </div>
  );
}
