import { useMemo } from 'react';
import { useNavigate } from 'react-router';
import { GitBranch } from 'lucide-react';
import { AppHeader } from '@/components/layout/app-header';
import { Badge } from '@/components/ui/badge';
import { useSSE } from '@/hooks/use-sse';
import { statusColors } from '@/lib/utils';

export function WorktreesPage() {
  const { sessions } = useSSE();
  const navigate = useNavigate();

  const worktreeSessions = useMemo(() => sessions.filter((s) => s.worktree_path), [sessions]);

  return (
    <div data-testid="worktrees-page">
      <AppHeader title="Worktrees" />
      <div className="space-y-4 p-4 sm:p-6">
        {worktreeSessions.length === 0 ? (
          <div className="py-12 text-center" data-testid="empty-state">
            <GitBranch className="mx-auto mb-3 h-10 w-10 text-muted-foreground" />
            <p className="text-muted-foreground">No worktree sessions</p>
            <p className="mt-1 text-sm text-muted-foreground">
              Use <code className="rounded bg-muted px-1.5 py-0.5">--worktree</code> when spawning
              to isolate agents in git worktrees.
            </p>
          </div>
        ) : (
          <div className="overflow-x-auto rounded-lg border border-border">
            <table className="w-full text-sm" data-testid="worktree-table">
              <thead>
                <tr className="border-b border-border bg-muted/50 text-left text-xs text-muted-foreground">
                  <th className="px-4 py-2.5 font-medium">Session</th>
                  <th className="px-4 py-2.5 font-medium">Branch</th>
                  <th className="px-4 py-2.5 font-medium">Status</th>
                  <th className="hidden px-4 py-2.5 font-medium sm:table-cell">Path</th>
                </tr>
              </thead>
              <tbody>
                {worktreeSessions.map((s) => (
                  <tr
                    key={s.id}
                    data-testid={`wt-row-${s.name}`}
                    className="cursor-pointer border-b border-border last:border-0 hover:bg-muted/30"
                    onClick={() => navigate(`/sessions/${s.id}`)}
                  >
                    <td className="px-4 py-3 font-medium">{s.name}</td>
                    <td className="px-4 py-3">
                      <span className="flex items-center gap-1 font-mono text-xs">
                        <GitBranch className="h-3.5 w-3.5 text-muted-foreground" />
                        {s.worktree_branch ?? s.worktree_path?.split('/').pop()}
                      </span>
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-1.5">
                        <span
                          className={`h-2 w-2 rounded-full ${statusColors[s.status] ?? 'bg-muted'}`}
                        />
                        <Badge variant="outline" className="text-xs uppercase">
                          {s.status}
                        </Badge>
                      </div>
                    </td>
                    <td className="hidden px-4 py-3 font-mono text-xs text-muted-foreground sm:table-cell">
                      {s.worktree_path}
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
