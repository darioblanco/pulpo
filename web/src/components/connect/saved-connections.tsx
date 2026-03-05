import { Button } from '@/components/ui/button';
import type { SavedConnection } from '@/hooks/use-connection';

interface SavedConnectionsProps {
  connections: SavedConnection[];
  onSelect: (conn: SavedConnection) => void;
  onRemove: (url: string) => void;
}

export function SavedConnections({ connections, onSelect, onRemove }: SavedConnectionsProps) {
  if (connections.length === 0) return null;

  return (
    <div data-testid="saved-connections" className="space-y-2">
      <h3 className="text-sm font-semibold text-muted-foreground">Saved connections</h3>
      {connections.map((conn) => (
        <div
          key={conn.url}
          data-testid={`saved-${conn.name}`}
          className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border px-3 py-2"
        >
          <button
            type="button"
            className="flex min-w-0 flex-col items-start text-left"
            data-testid={`select-${conn.name}`}
            onClick={() => onSelect(conn)}
          >
            <span className="font-medium">{conn.name}</span>
            <span className="truncate text-xs text-muted-foreground">{conn.url}</span>
          </button>
          <Button
            variant="outline"
            size="sm"
            data-testid={`remove-${conn.name}`}
            onClick={() => onRemove(conn.url)}
          >
            Remove
          </Button>
        </div>
      ))}
    </div>
  );
}
