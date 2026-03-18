import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { Button } from '@/components/ui/button';
import { getSchedules, updateSchedule, deleteSchedule } from '@/api/client';
import { toast } from 'sonner';
import type { ScheduleInfo } from '@/api/types';

export function SchedulesPage() {
  const [schedules, setSchedules] = useState<ScheduleInfo[]>([]);
  const [loading, setLoading] = useState(true);

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

  const handleToggle = async (schedule: ScheduleInfo) => {
    try {
      await updateSchedule(schedule.id, { enabled: !schedule.enabled });
      toast.success(`${schedule.name} ${schedule.enabled ? 'paused' : 'resumed'}`);
      fetchSchedules();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update');
    }
  };

  const handleDelete = async (schedule: ScheduleInfo) => {
    try {
      await deleteSchedule(schedule.id);
      toast.success(`Deleted "${schedule.name}"`);
      fetchSchedules();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to delete');
    }
  };

  return (
    <div data-testid="schedules-page">
      <AppHeader title="Schedules" />
      <div className="space-y-4 p-4 sm:p-6">
        {loading ? (
          <p className="text-center text-muted-foreground">Loading...</p>
        ) : schedules.length === 0 ? (
          <div className="py-12 text-center">
            <p className="text-muted-foreground">No schedules configured.</p>
            <p className="mt-1 text-sm text-muted-foreground">
              Create one with:{' '}
              <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                pulpo schedule add nightly &quot;0 3 * * *&quot; -- claude -p &quot;review&quot;
              </code>
            </p>
          </div>
        ) : (
          <div className="rounded-lg border">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-muted-foreground">
                  <th className="px-3 py-2 font-medium">Name</th>
                  <th className="px-3 py-2 font-medium">Cron</th>
                  <th className="hidden px-3 py-2 font-medium sm:table-cell">Node</th>
                  <th className="hidden px-3 py-2 font-medium md:table-cell">Last Run</th>
                  <th className="px-3 py-2 font-medium">Status</th>
                  <th className="px-3 py-2 font-medium">Actions</th>
                </tr>
              </thead>
              <tbody>
                {schedules.map((s) => (
                  <tr
                    key={s.id}
                    className={`border-b last:border-0 ${!s.enabled ? 'opacity-50' : ''}`}
                  >
                    <td className="px-3 py-2">
                      <div>
                        <span className="font-medium">{s.name}</span>
                        {s.description && (
                          <span className="ml-2 text-xs text-muted-foreground">
                            {s.description}
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 max-w-xs truncate text-xs text-muted-foreground">
                        {s.command || '(default)'}
                      </div>
                    </td>
                    <td className="px-3 py-2 font-mono text-xs">{s.cron}</td>
                    <td className="hidden px-3 py-2 sm:table-cell">{s.target_node ?? 'local'}</td>
                    <td className="hidden px-3 py-2 md:table-cell">
                      {s.last_run_at ? (
                        <span className="text-xs">{new Date(s.last_run_at).toLocaleString()}</span>
                      ) : (
                        <span className="text-xs text-muted-foreground">never</span>
                      )}
                    </td>
                    <td className="px-3 py-2">
                      <span
                        className={`text-xs ${s.enabled ? 'text-status-ready' : 'text-muted-foreground'}`}
                      >
                        {s.enabled ? 'active' : 'paused'}
                      </span>
                    </td>
                    <td className="px-3 py-2">
                      <div className="flex gap-1">
                        <Button variant="ghost" size="sm" onClick={() => handleToggle(s)}>
                          {s.enabled ? 'Pause' : 'Resume'}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-destructive"
                          onClick={() => handleDelete(s)}
                        >
                          Delete
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
