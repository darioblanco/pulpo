import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useNavigate } from 'react-router';
import { AppHeader } from '@/components/layout/app-header';
import { StatusSummary } from '@/components/dashboard/status-summary';
import { NodeCard } from '@/components/dashboard/node-card';
import { NewSessionDialog } from '@/components/dashboard/new-session-dialog';
import { SessionFilter } from '@/components/history/session-filter';
import { Skeleton } from '@/components/ui/skeleton';
import { getPeers, getSessions, cleanupSessions, stopSession } from '@/api/client';
import { Button } from '@/components/ui/button';
import { Trash2, CheckSquare } from 'lucide-react';
import { useSSE } from '@/hooks/use-sse';
import { useConnection } from '@/hooks/use-connection';
import { detectStatusChanges, showDesktopNotification } from '@/lib/notifications';

import { toast } from 'sonner';
import type { NodeInfo, Session } from '@/api/types';

const DEFAULT_STATUSES = new Set(['active', 'idle', 'ready']);

export function DashboardPage() {
  const navigate = useNavigate();
  const { isConnected } = useConnection();
  const { sessions, setSessions, connected } = useSSE();
  const [localNode, setLocalNode] = useState<NodeInfo | null>(null);
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
                <NewSessionDialog onCreated={handleSessionCreated} />
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
