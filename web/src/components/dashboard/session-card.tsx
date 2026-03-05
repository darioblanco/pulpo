import { useState } from 'react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { killSession, resumeSession, getInterventionEvents } from '@/api/client';
import type { Session, InterventionEvent } from '@/api/types';
import { OutputView } from '@/components/session/output-view';
import { TerminalView } from '@/components/session/terminal-view';

const statusColors: Record<string, string> = {
  running: 'bg-status-running',
  creating: 'bg-status-idle',
  completed: 'bg-status-completed',
  dead: 'bg-status-dead',
  stale: 'bg-status-stale',
};

interface SessionCardProps {
  session: Session;
  onRefresh: () => void;
}

export function SessionCard({ session, onRefresh }: SessionCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [interventionEvents, setInterventionEvents] = useState<InterventionEvent[]>([]);
  const [interventionsExpanded, setInterventionsExpanded] = useState(false);

  async function handleKill() {
    try {
      await killSession(session.id);
      onRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to kill session');
    }
  }

  async function handleResume() {
    try {
      await resumeSession(session.id);
      onRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to resume session');
    }
  }

  async function toggleInterventions() {
    if (!interventionsExpanded) {
      const events = await getInterventionEvents(session.id);
      setInterventionEvents(events);
    }
    setInterventionsExpanded(!interventionsExpanded);
  }

  return (
    <div className="mb-2 overflow-clip rounded-lg border border-border bg-card">
      <div
        data-testid="session-header"
        role="button"
        tabIndex={0}
        onClick={() => setExpanded(!expanded)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') setExpanded(!expanded);
        }}
        className="cursor-pointer px-4 py-3"
      >
        <div className="mb-1 flex flex-wrap items-center gap-x-2 gap-y-1">
          <span
            className={`h-2 w-2 shrink-0 rounded-full ${statusColors[session.status] ?? 'bg-muted'}`}
          />
          <strong className="min-w-0 truncate">{session.name}</strong>
          <span className="text-xs uppercase text-muted-foreground">{session.provider}</span>
          <Badge variant="outline" className="text-[0.625rem] uppercase">
            {session.mode}
          </Badge>
          {session.guard_config && (
            <Badge
              data-testid="guard-badge"
              variant="outline"
              className="text-[0.625rem] uppercase"
            >
              {session.guard_config.preset}
            </Badge>
          )}
          {session.status === 'dead' && session.intervention_reason && (
            <Badge
              data-testid="intervention-badge"
              variant="destructive"
              className="text-[0.625rem] uppercase"
            >
              intervened
            </Badge>
          )}
          <span className="ml-auto text-xs text-muted-foreground">{session.status}</span>
        </div>
        <p className="truncate text-sm text-muted-foreground">{session.prompt}</p>
      </div>

      {expanded && (
        <div className="border-t border-border px-4 pb-3">
          <div className="flex justify-between py-2 text-xs text-muted-foreground">
            <span>{session.workdir}</span>
            <span>{new Date(session.created_at).toLocaleString()}</span>
          </div>

          {session.status === 'running' && <TerminalView sessionId={session.id} />}

          {(session.status === 'stale' ||
            session.status === 'completed' ||
            session.status === 'dead') && (
            <OutputView sessionId={session.id} sessionStatus={session.status} />
          )}

          {session.status === 'dead' && session.intervention_reason && (
            <div className="mt-2 rounded-md border border-destructive/30 p-3">
              <p className="mb-1 text-sm font-medium text-destructive">
                Intervention: {session.intervention_reason}
              </p>
              {session.intervention_at && (
                <p className="text-xs text-muted-foreground">
                  {new Date(session.intervention_at).toLocaleString()}
                </p>
              )}
              <button
                data-testid="interventions-toggle"
                className="mt-1 cursor-pointer text-xs text-primary"
                onClick={toggleInterventions}
              >
                {interventionsExpanded ? 'Hide history' : 'Show history'}
              </button>
              {interventionsExpanded && interventionEvents.length > 0 && (
                <div data-testid="intervention-history" className="mt-2 space-y-1">
                  {interventionEvents.map((event) => (
                    <div key={event.id} className="border-l-2 border-destructive/50 pl-2 text-xs">
                      <span className="text-muted-foreground">
                        {new Date(event.created_at).toLocaleString()}
                      </span>
                      <span className="ml-1">{event.reason}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          <div className="mt-2 flex flex-wrap items-center gap-2">
            {session.status === 'running' && (
              <Button variant="outline" size="sm" className="text-destructive" onClick={handleKill}>
                Kill Session
              </Button>
            )}
            {session.status === 'stale' && (
              <>
                <Button size="sm" onClick={handleResume}>
                  Resume
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="text-destructive"
                  onClick={handleKill}
                >
                  Kill Session
                </Button>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
