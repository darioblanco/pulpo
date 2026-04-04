import { Fragment } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { cn, formatRelativeTime } from '@/lib/utils';
import { describeCron } from '@/lib/cron';
import { RunHistoryPanel } from '@/components/schedules/run-history-panel';
import { ChevronRight, ChevronDown, Pencil, Pause, Play, Trash2 } from 'lucide-react';
import type { ScheduleInfo } from '@/api/types';

interface Props {
  schedule: ScheduleInfo;
  nextRun: string | null;
  isExpanded: boolean;
  onToggleExpand: () => void;
  onEdit: () => void;
  onToggle: () => void;
  onDelete: () => void;
}

export function ScheduleRow({
  schedule,
  nextRun,
  isExpanded,
  onToggleExpand,
  onEdit,
  onToggle,
  onDelete,
}: Props) {
  const cronDesc = describeCron(schedule.cron);
  const rowClass = cn(
    'cursor-pointer border-b border-border/50 hover:bg-muted/40',
    schedule.enabled ? '' : 'opacity-50',
  );

  return (
    <Fragment>
      <tr
        data-testid={`schedule-row-${schedule.name}`}
        className={rowClass}
        onClick={onToggleExpand}
      >
        <td className="px-3 py-2">
          <div className="flex items-center gap-2">
            {isExpanded ? (
              <ChevronDown
                className="h-4 w-4 text-muted-foreground"
                data-testid={`chevron-down-${schedule.name}`}
              />
            ) : (
              <ChevronRight
                className="h-4 w-4 text-muted-foreground"
                data-testid={`chevron-right-${schedule.name}`}
              />
            )}
            <div>
              <div className="font-medium">{schedule.name}</div>
              {schedule.description && (
                <div className="text-xs text-muted-foreground">{schedule.description}</div>
              )}
              <div className="text-xs text-muted-foreground">
                {schedule.command || (schedule.ink ? `ink: ${schedule.ink}` : '(default)')}
              </div>
            </div>
          </div>
        </td>
        <td className="px-3 py-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                className="cursor-default font-mono text-xs"
                data-testid={`cron-${schedule.name}`}
              >
                {schedule.cron}
              </span>
            </TooltipTrigger>
            <TooltipContent>
              <p>{cronDesc}</p>
            </TooltipContent>
          </Tooltip>
        </td>
        <td className="hidden px-3 py-2 md:table-cell">
          {nextRun ? (
            <span className="text-xs">{nextRun}</span>
          ) : (
            <span className="text-xs text-muted-foreground">
              {schedule.enabled ? '--' : 'paused'}
            </span>
          )}
        </td>
        <td className="hidden px-3 py-2 sm:table-cell">
          {schedule.last_run_at ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="cursor-default text-xs">
                  {formatRelativeTime(schedule.last_run_at)}
                </span>
              </TooltipTrigger>
              <TooltipContent>
                <p>{new Date(schedule.last_run_at).toLocaleString()}</p>
              </TooltipContent>
            </Tooltip>
          ) : (
            <span className="text-xs text-muted-foreground">never</span>
          )}
          {schedule.last_error && (
            <p className="mt-0.5 truncate text-[11px] text-destructive">
              Failed{' '}
              {formatRelativeTime(
                schedule.last_attempted_at ?? schedule.last_run_at ?? schedule.created_at,
              )}
              : {schedule.last_error}
            </p>
          )}
        </td>
        <td className="px-3 py-2">
          {schedule.enabled ? (
            <Badge
              variant="outline"
              className="border-status-ready/30 bg-status-ready/10 text-status-ready"
              data-testid={`status-${schedule.name}`}
            >
              Active
            </Badge>
          ) : (
            <Badge variant="secondary" data-testid={`status-${schedule.name}`}>
              Paused
            </Badge>
          )}
        </td>
        <td className="px-3 py-2">
          <div className="flex justify-end gap-1" onClick={(event) => event.stopPropagation()}>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={onEdit}
                  data-testid={`edit-${schedule.name}`}
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
                  onClick={onToggle}
                  data-testid={`toggle-${schedule.name}`}
                  className="h-8 w-8 p-0"
                >
                  {schedule.enabled ? (
                    <Pause className="h-3.5 w-3.5" />
                  ) : (
                    <Play className="h-3.5 w-3.5" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent>{schedule.enabled ? 'Pause' : 'Resume'}</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={onDelete}
                  data-testid={`delete-${schedule.name}`}
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
      <RunHistoryPanel scheduleId={schedule.id} expanded={isExpanded} />
    </Fragment>
  );
}
