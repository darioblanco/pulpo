import { Card, CardContent } from '@/components/ui/card';
import type { Session } from '@/api/types';

interface StatusTile {
  label: string;
  count: number;
  color: string;
}

interface StatusSummaryProps {
  sessions: Session[];
}

export function StatusSummary({ sessions }: StatusSummaryProps) {
  const tiles: StatusTile[] = [
    {
      label: 'Running',
      count: sessions.filter((s) => s.status === 'running' || s.status === 'creating').length,
      color: 'text-status-running',
    },
    {
      label: 'Stale',
      count: sessions.filter((s) => s.status === 'stale').length,
      color: 'text-status-stale',
    },
    {
      label: 'Completed',
      count: sessions.filter((s) => s.status === 'completed').length,
      color: 'text-status-completed',
    },
    {
      label: 'Dead',
      count: sessions.filter((s) => s.status === 'dead').length,
      color: 'text-status-dead',
    },
  ];

  return (
    <div data-testid="status-summary" className="grid grid-cols-2 gap-3 sm:grid-cols-4">
      {tiles.map((tile) => (
        <Card key={tile.label}>
          <CardContent className="p-4">
            <p className="text-xs font-medium text-muted-foreground">{tile.label}</p>
            <p
              className={`text-2xl font-bold ${tile.color}`}
              data-testid={`count-${tile.label.toLowerCase()}`}
            >
              {tile.count}
            </p>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
