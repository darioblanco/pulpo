import { useState } from 'react';
import { toast } from 'sonner';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { killSession, resumeSession, getInterventionEvents } from '@/api/client';
import type { Session, InterventionEvent } from '@/api/types';
import { OutputView } from '@/components/session/output-view';
import { TerminalView } from '@/components/session/terminal-view';

interface SessionCardProps {
  session: Session;
  onRefresh: () => void;
}

function truncateCommand(command: string, maxLen = 40): string {
  if (command.length <= maxLen) return command;
  return command.slice(0, maxLen) + '...';
}

export function SessionCard({ session, onRefresh }: SessionCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [interventionEvents, setInterventionEvents] = useState<InterventionEvent[]>([]);
  const [interventionsExpanded, setInterventionsExpanded] = useState(false);

  const canKill = session.status === 'active' || session.status === 'lost';
  const canResume = session.status === 'lost';

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
    <div className="overflow-clip rounded-lg border border-[#1e2d3d]">
      {/* Terminal title bar */}
      <div
        data-testid="session-header"
        className="flex items-center gap-2 bg-[#0d1f33] px-3 py-1.5"
      >
        {/* Traffic lights */}
        <div className="flex items-center gap-1.5">
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <button
                data-testid="btn-kill"
                type="button"
                title={canKill ? 'Kill session' : 'Session not active'}
                disabled={!canKill}
                className={`flex items-center justify-center p-2 -m-1 ${canKill ? 'cursor-pointer' : ''}`}
              >
                <span
                  className={`block h-3 w-3 rounded-full ${canKill ? 'bg-[#ff5f57] hover:brightness-110' : 'bg-[#ff5f57]/30'}`}
                />
              </button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Kill session &quot;{session.name}&quot;?</AlertDialogTitle>
                <AlertDialogDescription>
                  This will terminate the session and stop the active agent. This action cannot be
                  undone.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  data-testid="btn-kill-confirm"
                  onClick={handleKill}
                  className="bg-destructive text-white hover:bg-destructive/90"
                >
                  Kill Session
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
          <button
            data-testid="btn-resume"
            type="button"
            title={canResume ? 'Resume session' : ''}
            disabled={!canResume}
            onClick={handleResume}
            className={`flex items-center justify-center p-2 -m-1 ${canResume ? 'cursor-pointer' : ''}`}
          >
            <span
              className={`block h-3 w-3 rounded-full ${canResume ? 'bg-[#febc2e] hover:brightness-110' : 'bg-[#febc2e]/30'}`}
            />
          </button>
          <button
            data-testid="btn-expand"
            type="button"
            title={expanded ? 'Collapse' : 'Expand'}
            onClick={() => setExpanded(!expanded)}
            className="flex cursor-pointer items-center justify-center p-2 -m-1"
          >
            <span className="block h-3 w-3 rounded-full bg-[#28c840] hover:brightness-110" />
          </button>
        </div>

        {/* Session info — clickable to expand */}
        <div
          role="button"
          tabIndex={0}
          onClick={() => setExpanded(!expanded)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') setExpanded(!expanded);
          }}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-x-2 overflow-hidden"
        >
          <strong className="shrink-0 font-mono text-xs text-[#c0d0e0]">{session.name}</strong>
          <span className="truncate max-w-[120px] sm:max-w-[200px] lg:max-w-none text-[0.6rem] uppercase text-[#5a7a9a]">
            {truncateCommand(session.command)}
          </span>
          {session.status === 'killed' && session.intervention_reason && (
            <Badge
              data-testid="intervention-badge"
              variant="destructive"
              className="text-[0.55rem] uppercase"
            >
              intervened
            </Badge>
          )}

          {/* Right side: metadata + status */}
          <span className="ml-auto flex shrink-0 items-center gap-2 font-mono text-[0.6rem] text-[#5a7a9a]">
            {session.ink && (
              <span data-testid="session-ink" className="truncate text-xs">
                {session.ink}
              </span>
            )}
            <span data-testid="session-workdir" className="hidden md:inline">
              {session.workdir.split('/').pop() || session.workdir}
            </span>
            <span data-testid="session-workdir-short" className="md:hidden">
              {session.workdir.split('/').pop() || session.workdir}
            </span>
            <span className="text-[#7a9aba]">{session.status}</span>
          </span>
        </div>
      </div>

      {/* Subtitle: description or command — always visible */}
      <div
        role="button"
        tabIndex={0}
        onClick={() => setExpanded(!expanded)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') setExpanded(!expanded);
        }}
        className="cursor-pointer border-t border-[#1e2d3d] bg-[#0d1f33]/60 px-3 py-1"
      >
        <p className="truncate font-mono text-xs text-[#5a7a9a]">
          {session.description || session.command}
        </p>
      </div>

      {/* Fullscreen terminal overlay (mobile only) */}
      {fullscreen && expanded && session.status === 'active' && (
        <div
          data-testid="fullscreen-terminal"
          className="fixed inset-0 z-50 flex flex-col bg-[#0a1628]"
        >
          <div className="flex items-center justify-between border-b border-[#1e2d3d] bg-[#0d1f33] px-3 py-2">
            <span className="font-mono text-xs text-[#c0d0e0]">{session.name}</span>
            <button
              data-testid="btn-fullscreen-close"
              type="button"
              onClick={() => setFullscreen(false)}
              className="cursor-pointer rounded px-2 py-1 text-xs text-[#c0d0e0] hover:bg-[#1e2d3d]"
            >
              Close
            </button>
          </div>
          <div className="min-h-0 flex-1">
            <TerminalView
              sessionId={session.id}
              className="h-full w-full min-w-0 overflow-hidden bg-[#0a1628]"
            />
          </div>
        </div>
      )}

      {/* Expanded body */}
      {expanded && (
        <div className="bg-[#0a1628]">
          {session.status === 'active' && (
            <div className="relative">
              <TerminalView sessionId={session.id} />
              <button
                data-testid="btn-fullscreen"
                type="button"
                onClick={() => setFullscreen(true)}
                className="absolute right-2 top-2 cursor-pointer rounded bg-[#1e2d3d] px-2 py-1 text-xs text-[#c0d0e0] hover:bg-[#2a3d4d] sm:hidden"
              >
                Fullscreen
              </button>
            </div>
          )}

          {(session.status === 'lost' ||
            session.status === 'ready' ||
            session.status === 'killed') && (
            <OutputView sessionId={session.id} sessionStatus={session.status} />
          )}

          {session.status === 'killed' && session.intervention_reason && (
            <div className="mx-3 mb-2 rounded-md border border-destructive/30 p-3">
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
        </div>
      )}
    </div>
  );
}
