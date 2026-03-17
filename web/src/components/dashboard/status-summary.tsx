import type { Session } from '@/api/types';

interface StatusSummaryProps {
  sessions: Session[];
}

export function StatusSummary({ sessions }: StatusSummaryProps) {
  const active = sessions.filter((s) => s.status === 'active' || s.status === 'creating').length;
  const idle = sessions.filter((s) => s.status === 'idle').length;
  const lost = sessions.filter((s) => s.status === 'lost').length;
  const ready = sessions.filter((s) => s.status === 'ready').length;
  const killed = sessions.filter((s) => s.status === 'killed').length;

  return (
    <div
      data-testid="status-summary"
      className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm"
    >
      <StatusDot color="bg-status-active" label="active" count={active} testId="count-active" />
      <StatusDot color="bg-status-idle" label="idle" count={idle} testId="count-idle" />
      <StatusDot color="bg-status-lost" label="lost" count={lost} testId="count-lost" />
      <StatusDot color="bg-status-ready" label="done" count={ready} testId="count-ready" />
      <StatusDot color="bg-status-killed" label="killed" count={killed} testId="count-killed" />
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
