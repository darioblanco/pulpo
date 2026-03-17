import { Badge } from '@/components/ui/badge';
import type { NodeInfo, Session } from '@/api/types';
import { SessionCard } from './session-card';

const statusDotColors: Record<string, string> = {
  online: 'bg-status-ready',
  offline: 'bg-status-killed',
  unknown: 'bg-muted-foreground',
};

interface NodeCardProps {
  name: string;
  nodeInfo: NodeInfo | null;
  status: 'online' | 'offline' | 'unknown';
  sessions: Session[];
  isLocal?: boolean;
  onRefresh: () => void;
}

export function NodeCard({
  name,
  nodeInfo,
  status,
  sessions,
  isLocal = false,
  onRefresh,
}: NodeCardProps) {
  return (
    <div data-testid="node-card" className={status !== 'online' ? 'opacity-50' : ''}>
      <div className="mb-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
        <span className={`h-2 w-2 shrink-0 rounded-full ${statusDotColors[status]}`} />
        <span className="font-medium">{name}</span>
        {isLocal && (
          <Badge variant="outline" className="text-[0.625rem] uppercase text-primary">
            local
          </Badge>
        )}
        {nodeInfo && (
          <span className="text-xs text-muted-foreground">
            {nodeInfo.os} · {nodeInfo.arch} · {nodeInfo.cpus} cores
          </span>
        )}
      </div>

      {status === 'online' ? (
        sessions.length === 0 ? (
          <p className="py-4 text-center text-sm text-muted-foreground">
            No active sessions on this node.
          </p>
        ) : (
          <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
            {sessions.map((session) => (
              <SessionCard key={session.id} session={session} onRefresh={onRefresh} />
            ))}
          </div>
        )
      ) : (
        <p className="py-4 text-center text-sm italic text-muted-foreground">
          Node is {status} — cannot fetch sessions.
        </p>
      )}
    </div>
  );
}
