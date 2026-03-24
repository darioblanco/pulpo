import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router';
import { toast } from 'sonner';
import { AppHeader } from '@/components/layout/app-header';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { TerminalView } from '@/components/session/terminal-view';
import { OutputView } from '@/components/session/output-view';
import {
  getSession,
  getInterventionEvents,
  stopSession,
  resumeSession,
  downloadSessionOutput,
} from '@/api/client';
import { formatRelativeTime, statusColors } from '@/lib/utils';
import type { Session, InterventionEvent } from '@/api/types';

export function SessionDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [session, setSession] = useState<Session | null>(null);
  const [interventions, setInterventions] = useState<InterventionEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchSession = useCallback(async () => {
    if (!id) return;
    try {
      const data = await getSession(id);
      setSession(data);
      setError(null);
    } catch {
      setError('Failed to load session');
    } finally {
      setLoading(false);
    }
  }, [id]);

  const fetchInterventions = useCallback(async () => {
    if (!id) return;
    try {
      const data = await getInterventionEvents(id);
      setInterventions(data);
    } catch {
      // Silently ignore
    }
  }, [id]);

  useEffect(() => {
    fetchSession();
    fetchInterventions();
  }, [fetchSession, fetchInterventions]);

  async function handleStop() {
    if (!id) return;
    try {
      await stopSession(id);
      toast.success('Session stopped');
      fetchSession();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to stop session');
    }
  }

  async function handleResume() {
    if (!id) return;
    try {
      await resumeSession(id);
      toast.success('Session resumed');
      fetchSession();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to resume session');
    }
  }

  async function handlePurge() {
    if (!id) return;
    try {
      await stopSession(id, true);
      toast.success('Session purged');
      navigate('/sessions');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to purge session');
    }
  }

  async function handleDownload() {
    if (!id || !session) return;
    try {
      const blob = await downloadSessionOutput(id);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `${session.name}.log`;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      toast.error('Failed to download logs');
    }
  }

  const canStop =
    session?.status === 'active' || session?.status === 'idle' || session?.status === 'lost';
  const canResume = session?.status === 'ready' || session?.status === 'lost';
  const canPurge = session?.status === 'stopped' || session?.status === 'lost';
  const showTerminal = session?.status === 'active' || session?.status === 'idle';
  const showOutput =
    session?.status === 'ready' || session?.status === 'stopped' || session?.status === 'lost';

  return (
    <div data-testid="session-detail-page">
      <AppHeader title={session?.name ?? 'Session'}>
        <Button variant="outline" size="sm" data-testid="btn-back" onClick={() => navigate(-1)}>
          Back
        </Button>
      </AppHeader>

      <div className="space-y-4 p-4 sm:p-6">
        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-4">
            <Skeleton className="h-20 w-full" />
            <Skeleton className="h-40 w-full" />
          </div>
        ) : error ? (
          <p className="py-8 text-center text-destructive">{error}</p>
        ) : session ? (
          <>
            {/* Header section */}
            <div className="flex flex-wrap items-center gap-3">
              <h2 className="text-2xl font-bold" data-testid="session-name">
                {session.name}
              </h2>
              <div className="flex items-center gap-1.5">
                <span
                  className={`h-2.5 w-2.5 rounded-full ${statusColors[session.status] ?? 'bg-muted'}`}
                />
                <Badge variant="outline" className="uppercase" data-testid="session-status">
                  {session.status}
                </Badge>
              </div>
              <div className="ml-auto flex flex-wrap gap-2">
                {canStop && (
                  <Button
                    variant="destructive"
                    size="sm"
                    data-testid="btn-stop"
                    onClick={handleStop}
                  >
                    Stop
                  </Button>
                )}
                {canResume && (
                  <Button
                    variant="outline"
                    size="sm"
                    data-testid="btn-resume"
                    onClick={handleResume}
                  >
                    Resume
                  </Button>
                )}
                {canPurge && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="text-destructive"
                    data-testid="btn-purge"
                    onClick={handlePurge}
                  >
                    Stop &amp; Purge
                  </Button>
                )}
                <Button
                  variant="outline"
                  size="sm"
                  data-testid="btn-download"
                  onClick={handleDownload}
                >
                  Download Logs
                </Button>
              </div>
            </div>

            {/* Info section */}
            <Card>
              <CardHeader>
                <CardTitle>Details</CardTitle>
              </CardHeader>
              <CardContent>
                <dl className="grid gap-3 text-sm sm:grid-cols-2">
                  <div>
                    <dt className="text-muted-foreground">Command</dt>
                    <dd className="font-mono text-xs break-all" data-testid="session-command">
                      {session.command}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Working Directory</dt>
                    <dd className="font-mono text-xs" data-testid="session-workdir">
                      {session.workdir}
                    </dd>
                  </div>
                  {session.ink && (
                    <div>
                      <dt className="text-muted-foreground">Ink</dt>
                      <dd data-testid="session-ink">{session.ink}</dd>
                    </div>
                  )}
                  {session.description && (
                    <div>
                      <dt className="text-muted-foreground">Description</dt>
                      <dd data-testid="session-description">{session.description}</dd>
                    </div>
                  )}
                  {session.worktree_branch && (
                    <div>
                      <dt className="text-muted-foreground">Worktree Branch</dt>
                      <dd className="font-mono text-xs" data-testid="session-worktree-branch">
                        {session.worktree_branch}
                      </dd>
                    </div>
                  )}
                  {session.worktree_path && (
                    <div>
                      <dt className="text-muted-foreground">Worktree Path</dt>
                      <dd className="font-mono text-xs" data-testid="session-worktree-path">
                        {session.worktree_path}
                      </dd>
                    </div>
                  )}
                  {session.git_branch && (
                    <div>
                      <dt className="text-muted-foreground">Git Branch</dt>
                      <dd className="font-mono text-xs" data-testid="session-git-branch">
                        {session.git_branch}
                      </dd>
                    </div>
                  )}
                  {session.git_commit && (
                    <div>
                      <dt className="text-muted-foreground">Git Commit</dt>
                      <dd className="font-mono text-xs" data-testid="session-git-commit">
                        {session.git_commit}
                      </dd>
                    </div>
                  )}
                  {(session.git_insertions != null || session.git_deletions != null) &&
                    ((session.git_insertions ?? 0) > 0 || (session.git_deletions ?? 0) > 0) && (
                      <div>
                        <dt className="text-muted-foreground">Git Diff</dt>
                        <dd className="font-mono text-xs" data-testid="session-git-diff">
                          <span className="text-green-400">+{session.git_insertions ?? 0}</span> /{' '}
                          <span className="text-red-400">-{session.git_deletions ?? 0}</span>
                          {session.git_files_changed != null && (
                            <span className="ml-1 text-muted-foreground">
                              ({session.git_files_changed} files)
                            </span>
                          )}
                        </dd>
                      </div>
                    )}
                  {session.git_ahead != null && session.git_ahead > 0 && (
                    <div>
                      <dt className="text-muted-foreground">Commits Ahead</dt>
                      <dd className="font-mono text-xs" data-testid="session-git-ahead">
                        {session.git_ahead}
                      </dd>
                    </div>
                  )}
                  {session.metadata?.error_status && (
                    <div>
                      <dt className="text-muted-foreground">Error</dt>
                      <dd className="font-mono text-xs text-red-400" data-testid="session-error">
                        {session.metadata.error_status}
                      </dd>
                    </div>
                  )}
                  {session.metadata?.total_input_tokens && (
                    <div>
                      <dt className="text-muted-foreground">Token Usage</dt>
                      <dd className="font-mono text-xs" data-testid="session-tokens">
                        Input: {Number(session.metadata.total_input_tokens).toLocaleString()}
                        {session.metadata.total_output_tokens && (
                          <span>
                            {' '}
                            / Output:{' '}
                            {Number(session.metadata.total_output_tokens).toLocaleString()}
                          </span>
                        )}
                      </dd>
                    </div>
                  )}
                  <div>
                    <dt className="text-muted-foreground">Created</dt>
                    <dd data-testid="session-created">{formatRelativeTime(session.created_at)}</dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Session ID</dt>
                    <dd
                      className="cursor-pointer font-mono text-xs text-muted-foreground"
                      data-testid="session-id"
                      title="Click to copy"
                      onClick={() => {
                        navigator.clipboard.writeText(session.id);
                        toast.success('Copied session ID');
                      }}
                    >
                      {session.id}
                    </dd>
                  </div>
                </dl>
              </CardContent>
            </Card>

            {/* Terminal / Output section */}
            {showTerminal && (
              <div data-testid="terminal-section">
                <TerminalView
                  sessionId={session.id}
                  className="h-[60vh] min-h-[300px] w-full min-w-0 resize-y overflow-hidden rounded-lg border border-border bg-[#0a1628]"
                />
              </div>
            )}
            {showOutput && (
              <div
                data-testid="output-section"
                className="overflow-hidden rounded-lg border border-border"
              >
                <OutputView sessionId={session.id} sessionStatus={session.status} />
              </div>
            )}

            {/* Intervention history section */}
            <Card>
              <CardHeader>
                <CardTitle>Intervention History</CardTitle>
              </CardHeader>
              <CardContent>
                {session.intervention_reason && (
                  <div
                    className="mb-4 rounded-md border border-destructive/30 p-3"
                    data-testid="latest-intervention"
                  >
                    <p className="text-sm font-medium text-destructive">
                      {session.intervention_reason}
                    </p>
                    {session.intervention_at && (
                      <p className="text-xs text-muted-foreground">
                        {formatRelativeTime(session.intervention_at)}
                      </p>
                    )}
                  </div>
                )}
                {interventions.length > 0 ? (
                  <div data-testid="intervention-list" className="space-y-2">
                    {interventions.map((event) => (
                      <div key={event.id} className="border-l-2 border-destructive/50 pl-3 text-sm">
                        <p>{event.reason}</p>
                        <p className="text-xs text-muted-foreground">
                          {formatRelativeTime(event.created_at)}
                        </p>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="text-sm text-muted-foreground" data-testid="no-interventions">
                    No interventions
                  </p>
                )}
              </CardContent>
            </Card>
          </>
        ) : null}
      </div>
    </div>
  );
}
