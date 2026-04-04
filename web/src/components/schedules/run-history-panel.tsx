import { useEffect, useState } from 'react';
import type { Session } from '@/api/types';
import { getScheduleRuns } from '@/api/client';
import { formatDuration, formatRelativeTime, isTerminal, statusColors } from '@/lib/utils';
import { Skeleton } from '@/components/ui/skeleton';
import { toast } from 'sonner';

interface Props {
  scheduleId: string;
  expanded: boolean;
}

export function RunHistoryPanel({ scheduleId, expanded }: Props) {
  const [runs, setRuns] = useState<Session[]>([]);
  const [loading, setLoading] = useState(false);
  const [fetched, setFetched] = useState(false);

  useEffect(() => {
    if (!expanded || fetched) return;
    setLoading(true);
    getScheduleRuns(scheduleId)
      .then((data) => {
        setRuns(data);
        setFetched(true);
      })
      .catch(() => {
        toast.error('Failed to load run history');
      })
      .finally(() => setLoading(false));
  }, [expanded, fetched, scheduleId]);

  if (!expanded) return null;
  if (loading) {
    return (
      <tr data-testid={`runs-loading-${scheduleId}`}>
        <td colSpan={6} className="px-6 py-3">
          <div className="space-y-1">
            <Skeleton className="h-6 w-full" />
            <Skeleton className="h-6 w-full" />
          </div>
        </td>
      </tr>
    );
  }

  if (runs.length === 0) {
    return (
      <tr data-testid={`runs-empty-${scheduleId}`}>
        <td colSpan={6} className="px-6 py-3 text-center text-sm text-muted-foreground">
          No runs yet
        </td>
      </tr>
    );
  }

  return (
    <tr data-testid={`runs-panel-${scheduleId}`}>
      <td colSpan={6} className="bg-muted/30 px-3 py-2">
        <table className="w-full text-xs">
          <thead>
            <tr className="text-left text-muted-foreground">
              <th className="px-2 py-1 font-medium">Session</th>
              <th className="px-2 py-1 font-medium">Status</th>
              <th className="px-2 py-1 font-medium">Created</th>
              <th className="px-2 py-1 font-medium">Duration</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((run) => (
              <tr key={run.id} className="border-t border-border/50" data-testid={`run-${run.id}`}>
                <td className="px-2 py-1">
                  <span className="font-medium text-foreground">{run.name}</span>
                </td>
                <td className="px-2 py-1">
                  <span className="inline-flex items-center gap-1.5">
                    <span
                      className={`inline-block h-2 w-2 rounded-full ${
                        statusColors[run.status] ?? 'bg-muted-foreground'
                      }`}
                    />
                    <span className="capitalize">{run.status}</span>
                  </span>
                </td>
                <td className="px-2 py-1">{formatRelativeTime(run.created_at)}</td>
                <td className="px-2 py-1">
                  {isTerminal(run.status) && run.updated_at
                    ? formatDuration(run.created_at, run.updated_at)
                    : formatDuration(run.created_at)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </td>
    </tr>
  );
}
