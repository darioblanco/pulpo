import { useState } from 'react';
import { useNavigate } from 'react-router';
import { toast } from 'sonner';
import {
  GitBranch,
  GitCommit,
  GitPullRequest,
  ExternalLink,
  AlertTriangle,
  ArrowUp,
  Coins,
  DollarSign,
  XCircle,
} from 'lucide-react';
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
import { stopSession, resumeSession, getInterventionEvents, sendInput } from '@/api/client';
import type { Session, InterventionEvent } from '@/api/types';
import { OutputView } from '@/components/session/output-view';
import { TerminalView } from '@/components/session/terminal-view';

interface SessionCardProps {
  session: Session;
  onRefresh: () => void;
  selectionMode?: boolean;
  selected?: boolean;
  onToggleSelect?: (id: string) => void;
}

function truncateCommand(command: string, maxLen = 40): string {
  if (command.length <= maxLen) return command;
  return command.slice(0, maxLen) + '...';
}

export function SessionCard({
  session,
  onRefresh,
  selectionMode,
  selected,
  onToggleSelect,
}: SessionCardProps) {
  const navigate = useNavigate();
  const [expanded, setExpanded] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [viewMode, setViewMode] = useState<'output' | 'terminal'>('output');
  const [interventionEvents, setInterventionEvents] = useState<InterventionEvent[]>([]);
  const [interventionsExpanded, setInterventionsExpanded] = useState(false);

  const canStop =
    session.status === 'active' || session.status === 'idle' || session.status === 'lost';
  const canResume = session.status === 'ready' || session.status === 'lost';

  async function handleStop() {
    try {
      await stopSession(session.id);
      onRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to stop session');
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
    <div className="relative overflow-clip rounded-lg border border-[#1e2d3d]">
      {selectionMode && (
        <label
          data-testid="session-checkbox"
          className="absolute left-1.5 top-1.5 z-10 flex cursor-pointer items-center"
        >
          <input
            type="checkbox"
            checked={!!selected}
            onChange={() => onToggleSelect?.(session.id)}
            className="h-4 w-4 rounded border-[#1e2d3d] accent-primary"
          />
        </label>
      )}
      {/* Terminal title bar */}
      <div
        data-testid="session-header"
        className={`flex items-center gap-2 bg-[#0d1f33] px-3 py-1.5 ${selectionMode ? 'pl-8' : ''}`}
      >
        {/* Traffic lights */}
        <div className="flex items-center gap-1.5">
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <button
                data-testid="btn-stop"
                type="button"
                title={canStop ? 'Stop session' : 'Session not active'}
                disabled={!canStop}
                className={`flex items-center justify-center p-2 -m-1 ${canStop ? 'cursor-pointer' : ''}`}
              >
                <span
                  className={`block h-3 w-3 rounded-full ${canStop ? 'bg-[#ff5f57] hover:brightness-110' : 'bg-[#ff5f57]/30'}`}
                />
              </button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Stop session &quot;{session.name}&quot;?</AlertDialogTitle>
                <AlertDialogDescription>
                  This will terminate the session and stop the active agent. This action cannot be
                  undone.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  data-testid="btn-stop-confirm"
                  onClick={handleStop}
                  className="bg-destructive text-white hover:bg-destructive/90"
                >
                  Stop Session
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
          className="flex min-w-0 flex-1 cursor-pointer flex-wrap items-center gap-x-2 gap-y-1 overflow-hidden"
        >
          <strong
            className="shrink-0 cursor-pointer font-mono text-xs text-[#c0d0e0] hover:underline"
            data-testid="session-name-link"
            onClick={(e) => {
              e.stopPropagation();
              navigate(`/sessions/${session.id}`);
            }}
          >
            {session.name}
          </strong>
          {session.worktree_path && (
            <span
              data-testid="worktree-badge"
              className="flex shrink-0 items-center gap-0.5 rounded bg-[#1e2d3d] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#7a9aba]"
            >
              <GitBranch className="h-3 w-3" />
              {session.worktree_branch ?? session.worktree_path?.split('/').pop()}
            </span>
          )}
          {!session.worktree_path && session.git_branch && (
            <span
              data-testid="git-branch-badge"
              className="flex shrink-0 items-center gap-0.5 rounded bg-[#1e2d3d] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#7a9aba]"
            >
              <GitCommit className="h-3 w-3" />
              {session.git_branch}
              {session.git_commit && <span className="text-[#5a7a9a]">@{session.git_commit}</span>}
            </span>
          )}
          {session.metadata?.pr_url && (
            <a
              data-testid="pr-badge"
              href={session.metadata.pr_url}
              target="_blank"
              rel="noopener noreferrer"
              onClick={(e) => e.stopPropagation()}
              className="flex shrink-0 items-center gap-0.5 rounded bg-[#0d2818] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#4ade80] hover:bg-[#14532d]"
            >
              <GitPullRequest className="h-3 w-3" />
              PR
              <ExternalLink className="h-2.5 w-2.5" />
            </a>
          )}
          {session.metadata?.branch && (
            <span
              data-testid="branch-badge"
              className="flex shrink-0 items-center gap-0.5 rounded bg-[#1e2d3d] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#7a9aba]"
            >
              <GitBranch className="h-3 w-3" />
              {session.metadata.branch}
            </span>
          )}
          {session.metadata?.auth_plan && (
            <span
              data-testid="auth-plan-badge"
              title={session.metadata.auth_email || undefined}
              className="shrink-0 rounded bg-[#1a1a2e] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#8b8bcd]"
            >
              {session.metadata.auth_plan}
            </span>
          )}
          {session.metadata?.rate_limit && (
            <span
              data-testid="rate-limit-badge"
              title={
                session.metadata.rate_limit_at
                  ? `${session.metadata.rate_limit} at ${new Date(session.metadata.rate_limit_at).toLocaleString()}`
                  : session.metadata.rate_limit
              }
              className={`flex shrink-0 items-center gap-0.5 rounded px-1.5 py-0.5 font-mono text-[0.55rem] ${
                session.metadata.rate_limit_at &&
                Date.now() - new Date(session.metadata.rate_limit_at).getTime() < 300_000
                  ? 'bg-[#3d2e0d] text-[#fbbf24]'
                  : 'bg-[#2d2a1e] text-[#a89a6a]'
              }`}
            >
              <AlertTriangle className="h-3 w-3" />
              {session.metadata.rate_limit}
            </span>
          )}
          {session.metadata?.error_status && (
            <span
              data-testid="error-status-badge"
              title={
                session.metadata.error_status_at
                  ? `${session.metadata.error_status} at ${new Date(session.metadata.error_status_at).toLocaleString()}`
                  : session.metadata.error_status
              }
              className="flex shrink-0 items-center gap-0.5 rounded bg-[#3d0d0d] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#f87171]"
            >
              <XCircle className="h-3 w-3" />
              {session.metadata.error_status}
            </span>
          )}
          {(session.git_insertions != null || session.git_deletions != null) &&
            ((session.git_insertions ?? 0) > 0 || (session.git_deletions ?? 0) > 0) && (
              <span
                data-testid="git-diff-badge"
                className="flex shrink-0 items-center gap-0.5 rounded bg-[#1e2d3d] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#7a9aba]"
              >
                <span className="text-[#4ade80]">+{session.git_insertions ?? 0}</span>/
                <span className="text-[#f87171]">-{session.git_deletions ?? 0}</span>
                {session.git_files_changed != null && (
                  <span className="text-[#5a7a9a]"> {session.git_files_changed}f</span>
                )}
              </span>
            )}
          {session.git_ahead != null && session.git_ahead > 0 && (
            <span
              data-testid="git-ahead-badge"
              className="flex shrink-0 items-center gap-0.5 rounded bg-[#1e2d3d] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#7a9aba]"
            >
              <ArrowUp className="h-3 w-3" />
              {session.git_ahead}
            </span>
          )}
          {session.metadata?.session_cost_usd && Number(session.metadata.session_cost_usd) > 0 ? (
            <span
              data-testid="cost-badge"
              className="flex shrink-0 items-center gap-0.5 rounded bg-[#1a2e1a] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#8bcd8b]"
              title={`Cost: $${Number(session.metadata.session_cost_usd).toFixed(2)}${session.metadata.total_input_tokens ? `, Input: ${Number(session.metadata.total_input_tokens).toLocaleString()}, Output: ${Number(session.metadata.total_output_tokens ?? '0').toLocaleString()}` : ''}`}
            >
              <DollarSign className="h-3 w-3" />$
              {Number(session.metadata.session_cost_usd).toFixed(2)}
            </span>
          ) : (
            session.metadata?.total_input_tokens && (
              <span
                data-testid="token-usage-badge"
                className="flex shrink-0 items-center gap-0.5 rounded bg-[#1a1a2e] px-1.5 py-0.5 font-mono text-[0.55rem] text-[#8b8bcd]"
                title={`Input: ${Number(session.metadata.total_input_tokens).toLocaleString()}, Output: ${Number(session.metadata.total_output_tokens ?? '0').toLocaleString()}${session.metadata.cache_write_tokens ? `, Cache Write: ${Number(session.metadata.cache_write_tokens).toLocaleString()}` : ''}${session.metadata.cache_read_tokens ? `, Cache Read: ${Number(session.metadata.cache_read_tokens).toLocaleString()}` : ''}`}
              >
                <Coins className="h-3 w-3" />
                {Number(session.metadata.total_input_tokens).toLocaleString()}
              </span>
            )
          )}
          <span className="truncate max-w-[120px] sm:max-w-[200px] lg:max-w-none text-[0.6rem] uppercase text-[#5a7a9a]">
            {truncateCommand(session.command)}
          </span>
          {session.status === 'stopped' && session.intervention_reason && (
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
        {session.status === 'idle' && session.output_snippet ? (
          <div>
            <p data-testid="idle-snippet" className="truncate font-mono text-xs text-[#febc2e]">
              {session.output_snippet.split('\n').filter(Boolean).slice(-2).join(' | ')}
            </p>
            <div data-testid="quick-reply-bar" className="mt-1 flex flex-wrap gap-1">
              {['yes', 'no', '1', '2', '3'].map((label) => (
                <button
                  key={label}
                  data-testid={`quick-reply-${label}`}
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    sendInput(session.id, label + '\n');
                    onRefresh();
                  }}
                  className="cursor-pointer rounded bg-[#1e2d3d] px-2 py-0.5 font-mono text-xs text-[#c0d0e0] hover:bg-[#2a3d4d]"
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <p className="truncate font-mono text-xs text-[#5a7a9a]">
            {session.description || session.command}
          </p>
        )}
      </div>

      {/* Fullscreen terminal overlay (mobile only) */}
      {fullscreen && expanded && (session.status === 'active' || session.status === 'idle') && (
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
          {(session.status === 'active' || session.status === 'idle') && (
            <div className="relative">
              <div className="flex justify-end px-2 pt-1">
                <button
                  data-testid="btn-view-toggle"
                  type="button"
                  onClick={() => setViewMode(viewMode === 'output' ? 'terminal' : 'output')}
                  className="cursor-pointer rounded bg-[#1e2d3d] px-2 py-0.5 text-[0.6rem] text-[#7a9aba] hover:bg-[#2a3d4d]"
                >
                  {viewMode === 'output' ? 'Terminal' : 'Output'}
                </button>
              </div>
              {viewMode === 'terminal' ? (
                <>
                  <TerminalView sessionId={session.id} />
                  <button
                    data-testid="btn-fullscreen"
                    type="button"
                    onClick={() => setFullscreen(true)}
                    className="absolute right-2 top-8 cursor-pointer rounded bg-[#1e2d3d] px-2 py-1 text-xs text-[#c0d0e0] hover:bg-[#2a3d4d] sm:hidden"
                  >
                    Fullscreen
                  </button>
                </>
              ) : (
                <OutputView sessionId={session.id} sessionStatus={session.status} />
              )}
            </div>
          )}

          {(session.status === 'lost' ||
            session.status === 'ready' ||
            session.status === 'stopped') && (
            <OutputView sessionId={session.id} sessionStatus={session.status} />
          )}

          {(session.status === 'stopped' || session.status === 'lost') && session.worktree_path && (
            <p
              data-testid="worktree-cleaned"
              className="mx-3 mb-2 font-mono text-xs text-muted-foreground"
            >
              Worktree cleaned up
            </p>
          )}

          {session.status === 'stopped' && session.intervention_reason && (
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
