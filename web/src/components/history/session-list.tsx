import { useState } from 'react';
import { useNavigate } from 'react-router';
import { GitBranch } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { deleteSession, downloadSessionOutput } from '@/api/client';
import type { Session } from '@/api/types';
import { statusColors } from '@/lib/utils';

interface SessionListProps {
  sessions: Session[];
  onRefresh: () => void;
}

export function SessionList({ sessions, onRefresh }: SessionListProps) {
  const navigate = useNavigate();
  const [expandedId, setExpandedId] = useState<string | null>(null);

  function toggleExpand(id: string) {
    setExpandedId(expandedId === id ? null : id);
  }

  async function handleDownload(session: Session) {
    const blob = await downloadSessionOutput(session.id);
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${session.name}.log`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function handleDelete(id: string) {
    await deleteSession(id);
    onRefresh();
  }

  if (sessions.length === 0) {
    return (
      <p data-testid="empty-message" className="py-8 text-center text-sm text-muted-foreground">
        No sessions found.
      </p>
    );
  }

  return (
    <div data-testid="session-list" className="space-y-2">
      {sessions.map((session) => (
        <div key={session.id} className="overflow-hidden rounded-lg border border-border bg-card">
          <div
            data-testid={`history-item-${session.id}`}
            role="button"
            tabIndex={0}
            onClick={() => toggleExpand(session.id)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') toggleExpand(session.id);
            }}
            className="flex cursor-pointer items-center gap-3 px-4 py-3"
          >
            <span
              className={`h-2 w-2 shrink-0 rounded-full ${statusColors[session.status] ?? 'bg-muted'}`}
            />
            <div className="min-w-0 flex-1">
              <strong
                className="cursor-pointer hover:underline"
                data-testid={`session-name-link-${session.id}`}
                onClick={(e) => {
                  e.stopPropagation();
                  navigate(`/sessions/${session.id}`);
                }}
              >
                {session.name}
              </strong>
              <p className="truncate text-sm text-muted-foreground">
                {(session.description || session.command).length > 80
                  ? (session.description || session.command).slice(0, 80) + '...'
                  : session.description || session.command}
              </p>
            </div>
            <Badge variant="outline" className="text-[0.625rem] uppercase">
              {session.status}
            </Badge>
          </div>

          {expandedId === session.id && (
            <div
              data-testid={`history-detail-${session.id}`}
              className="border-t border-border px-4 pb-3"
            >
              <div className="space-y-1 py-2 text-sm">
                <p>
                  <span className="text-muted-foreground">Command:</span> {session.command}
                </p>
                <p>
                  <span className="text-muted-foreground">Created:</span>{' '}
                  {new Date(session.created_at).toLocaleString()}
                </p>
                {session.worktree_path && (
                  <p className="flex items-center gap-1">
                    <GitBranch className="inline h-3 w-3 text-muted-foreground" />
                    <span className="text-muted-foreground">Worktree:</span> {session.worktree_path}
                  </p>
                )}
                {session.description && (
                  <p>
                    <span className="text-muted-foreground">Description:</span>{' '}
                    {session.description}
                  </p>
                )}
              </div>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  data-testid={`download-${session.id}`}
                  onClick={() => handleDownload(session)}
                >
                  Download Log
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="text-destructive"
                  data-testid={`delete-${session.id}`}
                  onClick={() => handleDelete(session.id)}
                >
                  Delete
                </Button>
              </div>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
