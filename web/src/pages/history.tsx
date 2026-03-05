import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { SessionFilter } from '@/components/history/session-filter';
import { SessionList } from '@/components/history/session-list';
import { Skeleton } from '@/components/ui/skeleton';
import { getSessions } from '@/api/client';
import type { Session, ListSessionsParams } from '@/api/types';

export function HistoryPage() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<ListSessionsParams>({ status: 'completed,dead' });

  const fetchSessions = useCallback(async (params: ListSessionsParams) => {
    setLoading(true);
    try {
      const data = await getSessions({
        ...params,
        status: params.status || 'completed,dead',
      });
      setSessions(data);
      setError(null);
    } catch {
      setError('Failed to load sessions');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchSessions(filter);
  }, [filter, fetchSessions]);

  function handleFilter(query: ListSessionsParams) {
    setFilter(query);
  }

  return (
    <div data-testid="history-page">
      <AppHeader title="History" />
      <div className="space-y-4 p-4 sm:p-6">
        <SessionFilter onFilter={handleFilter} />

        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-2">
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
          </div>
        ) : error ? (
          <p className="py-8 text-center text-destructive">{error}</p>
        ) : (
          <SessionList sessions={sessions} onRefresh={() => fetchSessions(filter)} />
        )}
      </div>
    </div>
  );
}
