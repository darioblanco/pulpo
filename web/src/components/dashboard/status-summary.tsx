import type { Session } from '@/api/types';

interface StatusSummaryProps {
  sessions: Session[];
}

export function StatusSummary({ sessions }: StatusSummaryProps) {
  const starting = sessions.filter((s) => s.status === 'starting').length;
  const working = sessions.filter((s) => s.status === 'working').length;
  const waiting = sessions.filter((s) => s.status === 'waiting').length;
  const done = sessions.filter((s) => s.status === 'done').length;
  const lost = sessions.filter((s) => s.status === 'lost').length;

  return (
    <div
      data-testid="status-summary"
      className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm"
    >
      <StatusDot
        color="bg-status-creating"
        label="starting"
        count={starting}
        testId="count-starting"
      />
      <StatusDot color="bg-status-active" label="working" count={working} testId="count-working" />
      <StatusDot color="bg-status-idle" label="waiting" count={waiting} testId="count-waiting" />
      <StatusDot color="bg-status-ready" label="done" count={done} testId="count-done" />
      <StatusDot color="bg-status-lost" label="lost" count={lost} testId="count-lost" />
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
