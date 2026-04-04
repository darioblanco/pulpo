import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { ScheduleDialog } from '@/components/schedules/schedule-dialog';
import { ScheduleRow } from '@/components/schedules/schedule-row';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { getSchedules, updateSchedule, deleteSchedule } from '@/api/client';
import { getNextRun } from '@/lib/cron';
import { toast } from 'sonner';
import { Plus, Search } from 'lucide-react';
import type { ScheduleInfo } from '@/api/types';
import { useSchedulesFilter } from '@/hooks/use-schedules-filter';

type StatusFilter = 'all' | 'active' | 'paused';

export function SchedulesPage() {
  const [schedules, setSchedules] = useState<ScheduleInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingSchedule, setEditingSchedule] = useState<ScheduleInfo | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ScheduleInfo | null>(null);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  const toggleExpanded = useCallback((id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const fetchSchedules = useCallback(async () => {
    try {
      const data = await getSchedules();
      setSchedules(data);
    } catch {
      toast.error('Failed to load schedules');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchSchedules();
  }, [fetchSchedules]);

  const wrapAction = useCallback(
    async (
      action: () => Promise<unknown>,
      successMessage: string,
      failureMessage: string,
      onSuccess?: () => void,
    ) => {
      try {
        await action();
        toast.success(successMessage);
        onSuccess?.();
        fetchSchedules();
      } catch (error) {
        const text = error instanceof Error ? error.message : failureMessage;
        toast.error(text);
      }
    },
    [fetchSchedules],
  );

  const handleToggle = useCallback(
    (schedule: ScheduleInfo) => {
      wrapAction(
        () => updateSchedule(schedule.id, { enabled: !schedule.enabled }),
        `${schedule.name} ${schedule.enabled ? 'paused' : 'resumed'}`,
        'Failed to update schedule',
      );
    },
    [wrapAction],
  );

  const handleDelete = useCallback(() => {
    if (!deleteTarget) return;
    wrapAction(
      () => deleteSchedule(deleteTarget.id),
      `Deleted "${deleteTarget.name}"`,
      'Failed to delete schedule',
      () => setDeleteTarget(null),
    );
  }, [deleteTarget, wrapAction]);

  const handleEdit = useCallback((schedule: ScheduleInfo) => {
    setEditingSchedule(schedule);
    setDialogOpen(true);
  }, []);

  const handleCreate = useCallback(() => {
    setEditingSchedule(null);
    setDialogOpen(true);
  }, []);

  const { filteredSchedules, statusCounts } = useSchedulesFilter(
    schedules,
    searchQuery,
    statusFilter,
  );

  return (
    <div className="space-y-6">
      <AppHeader title="Schedules" />

      <div className="grid gap-4 rounded-xl border border-border bg-card p-4 shadow-sm">
        <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
          <div className="flex flex-wrap items-center gap-2">
            <div className="flex items-center gap-2">
              <Search className="h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search schedules"
                value={searchQuery}
                data-testid="schedule-search-input"
                onChange={(event) => setSearchQuery(event.target.value)}
              />
            </div>
            <Button
              variant="secondary"
              onClick={() => setStatusFilter('all')}
              data-testid="filter-all"
            >
              all({statusCounts.all})
            </Button>
            <Button
              variant="secondary"
              onClick={() => setStatusFilter('active')}
              data-testid="filter-active"
            >
              active({statusCounts.active})
            </Button>
            <Button
              variant="secondary"
              onClick={() => setStatusFilter('paused')}
              data-testid="filter-paused"
            >
              paused({statusCounts.paused})
            </Button>
          </div>
          <Button data-testid="new-schedule-button" onClick={handleCreate}>
            <Plus className="h-3.5 w-3.5" />
            New Schedule
          </Button>
        </div>

        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-2 p-4">
            <Skeleton className="h-4 w-1/4" />
            <Skeleton className="h-4 w-1/2" />
            <Skeleton className="h-4 w-3/4" />
          </div>
        ) : filteredSchedules.length === 0 ? (
          <div
            data-testid="empty-state"
            className="space-y-2 px-4 py-12 text-center text-sm text-muted-foreground"
          >
            {schedules.length === 0 ? (
              <>
                <p>No schedules configured yet.</p>
                <p>
                  Schedules let you run sessions on a cron-based timer. Click{' '}
                  <strong>New Schedule</strong> to create one, or use the CLI:
                </p>
                <code className="mt-1 inline-block rounded bg-muted px-2 py-1 text-xs">
                  pulpo schedule add nightly "0 3 * * *" -- claude -p "review"
                </code>
              </>
            ) : (
              <p>No schedules match your filters.</p>
            )}
          </div>
        ) : (
          <div className="overflow-x-auto">
            <TooltipProvider>
              <table className="w-full text-left text-sm" data-testid="schedule-table">
                <thead>
                  <tr className="text-xs uppercase text-muted-foreground">
                    <th className="px-3 py-2">Schedule</th>
                    <th className="px-3 py-2">Cron</th>
                    <th className="hidden px-3 py-2 md:table-cell">Next Run</th>
                    <th className="hidden px-3 py-2 sm:table-cell">Last Run</th>
                    <th className="px-3 py-2">Status</th>
                    <th className="px-3 py-2">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredSchedules.map((schedule) => {
                    const nextRunDate = schedule.enabled ? getNextRun(schedule.cron) : null;
                    const nextRun = nextRunDate ? nextRunDate.toLocaleString() : null;
                    return (
                      <ScheduleRow
                        key={schedule.id}
                        schedule={schedule}
                        nextRun={nextRun}
                        isExpanded={expandedIds.has(schedule.id)}
                        onToggleExpand={() => toggleExpanded(schedule.id)}
                        onEdit={() => handleEdit(schedule)}
                        onToggle={() => handleToggle(schedule)}
                        onDelete={() => setDeleteTarget(schedule)}
                      />
                    );
                  })}
                </tbody>
              </table>
            </TooltipProvider>
          </div>
        )}
      </div>

      <ScheduleDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        schedule={editingSchedule}
        onSaved={fetchSchedules}
      />

      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete Schedule</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to delete "{deleteTarget?.name}"? This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={handleDelete}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
