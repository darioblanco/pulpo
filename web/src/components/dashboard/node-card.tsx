import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import type { NodeInfo, Session } from '@/api/types';
import { SessionCard } from './session-card';

const statusDotColors: Record<string, string> = {
  online: 'bg-status-completed',
  offline: 'bg-status-dead',
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
    <Card data-testid="node-card" className={status !== 'online' ? 'opacity-50' : ''}>
      <CardHeader className="pb-2">
        <CardTitle className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
          <span className={`h-2 w-2 shrink-0 rounded-full ${statusDotColors[status]}`} />
          <span className="min-w-0 truncate">{name}</span>
          {isLocal && (
            <Badge variant="outline" className="text-[0.625rem] uppercase text-primary">
              local
            </Badge>
          )}
          <span className="ml-auto text-xs font-normal text-muted-foreground">
            {sessions.length} session{sessions.length !== 1 ? 's' : ''}
          </span>
          {nodeInfo && (
            <span className="w-full text-xs font-normal text-muted-foreground">
              {nodeInfo.os} · {nodeInfo.arch} · {nodeInfo.cpus} cores
            </span>
          )}
        </CardTitle>
      </CardHeader>
      <CardContent>
        {status === 'online' ? (
          sessions.length === 0 ? (
            <p className="py-4 text-center text-sm text-muted-foreground">
              No active sessions on this node.
            </p>
          ) : (
            sessions.map((session) => (
              <SessionCard key={session.id} session={session} onRefresh={onRefresh} />
            ))
          )
        ) : (
          <p className="py-4 text-center text-sm italic text-muted-foreground">
            Node is {status} — cannot fetch sessions.
          </p>
        )}
      </CardContent>
    </Card>
  );
}
