import { Fragment, useState, useEffect, useCallback, useMemo } from 'react';
import { Link } from 'react-router';
import { AppHeader } from '@/components/layout/app-header';
import { ScheduleDialog } from '@/components/schedules/schedule-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
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
import { getSchedules, getScheduleRuns, updateSchedule, deleteSchedule } from '@/api/client';
import { describeCron, getNextRun } from '@/lib/cron';
import { formatRelativeTime, formatDuration, statusColors } from '@/lib/utils';
import { toast } from 'sonner';
import { Plus, Search, Pencil, Pause, Play, Trash2, ChevronRight, ChevronDown } from 'lucide-react';
import type { ScheduleInfo, Session } from '@/api/types';

type StatusFilter = 'all' | 'active' | 'paused';

function isTerminal(status: string): boolean {
  return status === 'stopped' || status === 'ready' || status === 'lost';
}

function RunHistoryPanel({ scheduleId, expanded }: { scheduleId: string; expanded: boolean }) {
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
        <td colSpan={7} className="px-6 py-3">
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
        <td colSpan={7} className="px-6 py-3 text-center text-sm text-muted-foreground">
          No runs yet
        </td>
      </tr>
    );
  }

  return (
    <tr data-testid={`runs-panel-${scheduleId}`}>
      <td colSpan={7} className="bg-muted/30 px-3 py-2">
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
                  <Link
                    to={`/sessions/${run.id}`}
                    className="font-medium text-foreground hover:underline"
                  >
                    {run.name}
                  </Link>
                </td>
                <td className="px-2 py-1">
                  <span className="inline-flex items-center gap-1.5">
                    <span
                      className={`inline-block h-2 w-2 rounded-full ${statusColors[run.status] ?? 'bg-muted-foreground'}`}
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

export function SchedulesPage() {
  const [schedules, setSchedules] = useState<ScheduleInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingSchedule, setEditingSchedule] = useState<ScheduleInfo | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ScheduleInfo | null>(null);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  const toggleExpanded = (id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

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

  const filteredSchedules = useMemo(() => {
    return schedules.filter((s) => {
      if (searchQuery && !s.name.toLowerCase().includes(searchQuery.toLowerCase())) {
        return false;
      }
      if (statusFilter === 'active' && !s.enabled) return false;
      if (statusFilter === 'paused' && s.enabled) return false;
      return true;
    });
  }, [schedules, searchQuery, statusFilter]);

  const handleToggle = async (schedule: ScheduleInfo) => {
    try {
      await updateSchedule(schedule.id, { enabled: !schedule.enabled });
      toast.success(`${schedule.name} ${schedule.enabled ? 'paused' : 'resumed'}`);
      fetchSchedules();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update');
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      await deleteSchedule(deleteTarget.id);
      toast.success(`Deleted "${deleteTarget.name}"`);
      setDeleteTarget(null);
      fetchSchedules();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete');
    }
  };

  function handleEdit(schedule: ScheduleInfo) {
    setEditingSchedule(schedule);
    setDialogOpen(true);
  }

  function handleCreate() {
    setEditingSchedule(null);
    setDialogOpen(true);
  }

  function handleDialogSaved() {
    fetchSchedules();
  }

  function formatNextRun(cron: string): string | null {
    const next = getNextRun(cron);
    if (!next) return null;
    return next.toLocaleString();
  }

  const statusCounts = useMemo(() => {
    const active = schedules.filter((s) => s.enabled).length;
    return { all: schedules.length, active, paused: schedules.length - active };
  }, [schedules]);

  return (
    <div data-testid="schedules-page">
      <AppHeader title="Schedules" />
      <div className="space-y-4 p-4 sm:p-6">
        {/* Toolbar */}
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="absolute top-1/2 left-2.5 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                placeholder="Search schedules..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-64 pl-9"
                data-testid="schedule-search-input"
              />
            </div>
            <div className="flex rounded-md border" data-testid="status-filter">
              {(['all', 'active', 'paused'] as const).map((status) => (
                <button
                  key={status}
                  onClick={() => setStatusFilter(status)}
                  className={`px-3 py-1.5 text-xs font-medium capitalize transition-colors ${
                    statusFilter === status
                      ? 'bg-secondary text-foreground'
                      : 'text-muted-foreground hover:text-foreground'
                  } ${status !== 'all' ? 'border-l' : ''}`}
                  data-testid={`filter-${status}`}
                >
                  {status}
                  <span className="ml-1 text-muted-foreground">({statusCounts[status]})</span>
                </button>
              ))}
            </div>
          </div>
          <Button onClick={handleCreate} data-testid="new-schedule-button">
            <Plus className="mr-2 h-4 w-4" />
            New Schedule
          </Button>
        </div>

        {/* Content */}
        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-2">
            <Skeleton className="h-12 w-full" />
            <Skeleton className="h-12 w-full" />
            <Skeleton className="h-12 w-full" />
          </div>
        ) : filteredSchedules.length === 0 ? (
          <div className="py-12 text-center" data-testid="empty-state">
            {schedules.length === 0 ? (
              <>
                <p className="text-muted-foreground">No schedules configured yet.</p>
                <p className="mt-2 text-sm text-muted-foreground">
                  Schedules let you run sessions on a cron-based timer. Click{' '}
                  <strong>New Schedule</strong> to create one, or use the CLI:
                </p>
                <code className="mt-1 inline-block rounded bg-muted px-2 py-1 text-xs">
                  pulpo schedule add nightly &quot;0 3 * * *&quot; -- claude -p &quot;review&quot;
                </code>
              </>
            ) : (
              <p className="text-muted-foreground">No schedules match your filters.</p>
            )}
          </div>
        ) : (
          <TooltipProvider>
            <div className="rounded-lg border" data-testid="schedule-table">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-left text-muted-foreground">
                    <th className="px-3 py-2 font-medium">Name</th>
                    <th className="px-3 py-2 font-medium">Cron</th>
                    <th className="hidden px-3 py-2 font-medium md:table-cell">Next Run</th>
                    <th className="hidden px-3 py-2 font-medium sm:table-cell">Last Run</th>
                    <th className="hidden px-3 py-2 font-medium lg:table-cell">Node</th>
                    <th className="px-3 py-2 font-medium">Status</th>
                    <th className="px-3 py-2 text-right font-medium">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredSchedules.map((s) => {
                    const nextRun = s.enabled ? formatNextRun(s.cron) : null;
                    const cronDesc = describeCron(s.cron);
                    const isExpanded = expandedIds.has(s.id);

                    return (
                      <Fragment key={s.id}>
                        <tr
                          className={`cursor-pointer border-b last:border-0 ${!s.enabled ? 'opacity-50' : ''}`}
                          data-testid={`schedule-row-${s.name}`}
                          onClick={() => toggleExpanded(s.id)}
                        >
                          {/* Name + command */}
                          <td className="px-3 py-2">
                            <div className="flex items-center gap-1.5">
                              {isExpanded ? (
                                <ChevronDown
                                  className="h-3.5 w-3.5 shrink-0 text-muted-foreground"
                                  data-testid={`chevron-down-${s.name}`}
                                />
                              ) : (
                                <ChevronRight
                                  className="h-3.5 w-3.5 shrink-0 text-muted-foreground"
                                  data-testid={`chevron-right-${s.name}`}
                                />
                              )}
                              <div>
                                <span className="font-medium">{s.name}</span>
                                {s.description && (
                                  <span className="ml-2 text-xs text-muted-foreground">
                                    {s.description}
                                  </span>
                                )}
                                <div className="mt-0.5 max-w-xs truncate text-xs text-muted-foreground">
                                  {s.command || (s.ink ? `ink: ${s.ink}` : '(default)')}
                                </div>
                              </div>
                            </div>
                          </td>

                          {/* Cron with tooltip */}
                          <td className="px-3 py-2">
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <span
                                  className="cursor-default font-mono text-xs"
                                  data-testid={`cron-${s.name}`}
                                >
                                  {s.cron}
                                </span>
                              </TooltipTrigger>
                              <TooltipContent>
                                <p>{cronDesc}</p>
                              </TooltipContent>
                            </Tooltip>
                          </td>

                          {/* Next Run */}
                          <td className="hidden px-3 py-2 md:table-cell">
                            {nextRun ? (
                              <span className="text-xs">{nextRun}</span>
                            ) : (
                              <span className="text-xs text-muted-foreground">
                                {s.enabled ? '--' : 'paused'}
                              </span>
                            )}
                          </td>

                          {/* Last Run */}
                          <td className="hidden px-3 py-2 sm:table-cell">
                            {s.last_run_at ? (
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <span className="cursor-default text-xs">
                                    {formatRelativeTime(s.last_run_at)}
                                  </span>
                                </TooltipTrigger>
                                <TooltipContent>
                                  <p>{new Date(s.last_run_at).toLocaleString()}</p>
                                </TooltipContent>
                              </Tooltip>
                            ) : (
                              <span className="text-xs text-muted-foreground">never</span>
                            )}
                          </td>

                          {/* Target Node */}
                          <td className="hidden px-3 py-2 lg:table-cell">
                            <span className="text-xs">{s.target_node ?? 'local'}</span>
                          </td>

                          {/* Status badge */}
                          <td className="px-3 py-2">
                            {s.enabled ? (
                              <Badge
                                variant="outline"
                                className="border-status-ready/30 bg-status-ready/10 text-status-ready"
                                data-testid={`status-${s.name}`}
                              >
                                Active
                              </Badge>
                            ) : (
                              <Badge variant="secondary" data-testid={`status-${s.name}`}>
                                Paused
                              </Badge>
                            )}
                          </td>

                          {/* Actions */}
                          <td className="px-3 py-2">
                            <div
                              className="flex justify-end gap-1"
                              onClick={(e) => e.stopPropagation()}
                            >
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => handleEdit(s)}
                                    data-testid={`edit-${s.name}`}
                                    className="h-8 w-8 p-0"
                                  >
                                    <Pencil className="h-3.5 w-3.5" />
                                  </Button>
                                </TooltipTrigger>
                                <TooltipContent>Edit</TooltipContent>
                              </Tooltip>
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => handleToggle(s)}
                                    data-testid={`toggle-${s.name}`}
                                    className="h-8 w-8 p-0"
                                  >
                                    {s.enabled ? (
                                      <Pause className="h-3.5 w-3.5" />
                                    ) : (
                                      <Play className="h-3.5 w-3.5" />
                                    )}
                                  </Button>
                                </TooltipTrigger>
                                <TooltipContent>{s.enabled ? 'Pause' : 'Resume'}</TooltipContent>
                              </Tooltip>
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => setDeleteTarget(s)}
                                    data-testid={`delete-${s.name}`}
                                    className="h-8 w-8 p-0 text-destructive hover:text-destructive"
                                  >
                                    <Trash2 className="h-3.5 w-3.5" />
                                  </Button>
                                </TooltipTrigger>
                                <TooltipContent>Delete</TooltipContent>
                              </Tooltip>
                            </div>
                          </td>
                        </tr>
                        <RunHistoryPanel scheduleId={s.id} expanded={isExpanded} />
                      </Fragment>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </TooltipProvider>
        )}
      </div>

      {/* Create/Edit Dialog */}
      <ScheduleDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        schedule={editingSchedule}
        onSaved={handleDialogSaved}
      />

      {/* Delete Confirmation */}
      <AlertDialog open={!!deleteTarget} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete Schedule</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to delete &quot;{deleteTarget?.name}&quot;? This action cannot
              be undone.
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
