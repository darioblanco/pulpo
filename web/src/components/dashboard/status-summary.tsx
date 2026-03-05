import type { Session } from '@/api/types';

interface StatusSummaryProps {
  sessions: Session[];
}

export function StatusSummary({ sessions }: StatusSummaryProps) {
  const running = sessions.filter(
    (s) => s.status === 'running' || s.status === 'creating',
  ).length;
  const stale = sessions.filter((s) => s.status === 'stale').length;
  const completed = sessions.filter((s) => s.status === 'completed').length;
  const dead = sessions.filter((s) => s.status === 'dead').length;

  return (
    <div data-testid="status-summary" className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
      <StatusDot color="bg-status-running" label="running" count={running} testId="count-running" />
      <StatusDot color="bg-status-stale" label="stale" count={stale} testId="count-stale" />
      <StatusDot
        color="bg-status-completed"
        label="done"
        count={completed}
        testId="count-completed"
      />
      <StatusDot color="bg-status-dead" label="dead" count={dead} testId="count-dead" />
    </div>
  );
}

function StatusDot({
  color,
  label,
  count,
  testId,
}: {
  color: string;
  label: string;
  count: number;
  testId: string;
}) {
  return (
    <span className="inline-flex items-center gap-1.5 text-muted-foreground">
      <span className={`h-2 w-2 shrink-0 rounded-full ${color}`} />
      <span data-testid={testId} className="font-medium tabular-nums text-foreground">
        {count}
      </span>
      {label}
    </span>
  );
}
