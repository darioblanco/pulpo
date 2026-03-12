import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { CheckCircle, Trash2 } from 'lucide-react';
import type { Culture } from '@/api/types';

interface CultureListProps {
  items: Culture[];
  onDelete: (id: string) => void;
  onApprove: (id: string) => void;
  onRefresh: () => void;
}

export function CultureList({ items, onDelete, onApprove }: CultureListProps) {
  return (
    <div className="space-y-3" data-testid="culture-list">
      <p className="text-xs text-muted-foreground">{items.length} items</p>
      {items.map((item) => (
        <CultureCard key={item.id} item={item} onDelete={onDelete} onApprove={onApprove} />
      ))}
    </div>
  );
}

function CultureCard({
  item,
  onDelete,
  onApprove,
}: {
  item: Culture;
  onDelete: (id: string) => void;
  onApprove: (id: string) => void;
}) {
  const date = new Date(item.created_at).toLocaleDateString();
  const isStale = item.tags.includes('stale');
  const isSuperseded = item.tags.includes('superseded');
  const isDimmed = isStale || isSuperseded;

  return (
    <Card data-testid="culture-card" className={isDimmed ? 'opacity-60' : ''}>
      <CardHeader className="flex flex-row items-start justify-between space-y-0 pb-2">
        <div className="space-y-1">
          <CardTitle className="text-sm font-medium">{item.title}</CardTitle>
          <div className="flex items-center gap-1.5">
            <Badge variant={item.kind === 'failure' ? 'destructive' : 'secondary'}>
              {item.kind}
            </Badge>
            {item.scope_repo && (
              <Badge variant="outline" className="font-mono text-xs">
                {item.scope_repo.split('/').pop()}
              </Badge>
            )}
            {item.scope_ink && <Badge variant="outline">{item.scope_ink}</Badge>}
            <span className="text-xs text-muted-foreground">{date}</span>
            <span className="text-xs text-muted-foreground">
              relevance: {item.relevance.toFixed(2)}
            </span>
          </div>
        </div>
        <div className="flex gap-1">
          {isStale && (
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 text-muted-foreground hover:text-green-600"
              onClick={() => onApprove(item.id)}
              data-testid="approve-culture-btn"
            >
              <CheckCircle className="h-3.5 w-3.5" />
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-destructive"
            onClick={() => onDelete(item.id)}
            data-testid="delete-culture-btn"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <p className="whitespace-pre-wrap text-sm text-muted-foreground">{item.body}</p>
        {item.tags.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {item.tags.map((tag) => (
              <Badge key={tag} variant="outline" className="text-xs">
                {tag}
              </Badge>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
