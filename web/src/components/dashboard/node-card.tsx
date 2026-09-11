import { Badge } from '@/components/ui/badge';
import type { NodeInfo, Session } from '@/api/types';
import { SessionCard } from './session-card';
import { formatMemory } from '@/lib/utils';

interface NodeCardProps {
  name: string;
  nodeInfo: NodeInfo | null;
  sessions: Session[];
  onRefresh: () => void;
  selectionMode?: boolean;
  selectedIds?: Set<string>;
  onToggleSelect?: (id: string) => void;
}

/** The local node's session list — pulpo is single-node-first, so this always
 * renders the machine `pulpod` is running on. */
export function NodeCard({
  name,
  nodeInfo,
  sessions,
  onRefresh,
  selectionMode,
  selectedIds,
  onToggleSelect,
}: NodeCardProps) {
  return (
    <div data-testid="node-card">
      <div className="mb-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
        <span className="h-2 w-2 shrink-0 rounded-full bg-status-ready" />
        <span className="font-medium">{name}</span>
        <Badge variant="outline" className="text-[0.625rem] uppercase text-primary">
          local
        </Badge>
      </div>

      {nodeInfo && (
        <div
          data-testid="node-info-bar"
          className="mb-3 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md border bg-muted/40 px-3 py-1.5 text-xs text-muted-foreground"
        >
          <span>{nodeInfo.hostname}</span>
          <span>
            {nodeInfo.os} {nodeInfo.arch}
          </span>
          <span>{nodeInfo.cpus} CPU</span>
          <span>{formatMemory(nodeInfo.memory_mb)}</span>
          {nodeInfo.gpu && <span>{nodeInfo.gpu}</span>}
        </div>
      )}

      {sessions.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">
          No active sessions on this node.
        </p>
      ) : (
        <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
          {sessions.map((session) => (
            <SessionCard
              key={session.id}
              session={session}
              onRefresh={onRefresh}
              selectionMode={selectionMode}
              selected={selectedIds?.has(session.id)}
              onToggleSelect={onToggleSelect}
            />
          ))}
        </div>
      )}
    </div>
  );
}
