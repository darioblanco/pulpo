import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { KnowledgeList } from '@/components/knowledge/knowledge-list';
import { KnowledgeFilter } from '@/components/knowledge/knowledge-filter';
import { Skeleton } from '@/components/ui/skeleton';
import { Button } from '@/components/ui/button';
import { listKnowledge, deleteKnowledge, pushKnowledge } from '@/api/client';
import type { Knowledge } from '@/api/types';
import { toast } from 'sonner';
import { Upload } from 'lucide-react';

export interface KnowledgeFilterParams {
  kind?: string;
  repo?: string;
  ink?: string;
}

export function KnowledgePage() {
  const [items, setItems] = useState<Knowledge[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<KnowledgeFilterParams>({});
  const [pushing, setPushing] = useState(false);

  const fetchKnowledge = useCallback(async (params: KnowledgeFilterParams) => {
    setLoading(true);
    try {
      const data = await listKnowledge({ ...params, limit: 100 });
      setItems(data.knowledge);
      setError(null);
    } catch {
      setError('Failed to load knowledge');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchKnowledge(filter);
  }, [filter, fetchKnowledge]);

  async function handleDelete(id: string) {
    try {
      await deleteKnowledge(id);
      setItems((prev) => prev.filter((k) => k.id !== id));
      toast.success('Knowledge item deleted');
    } catch {
      toast.error('Failed to delete knowledge item');
    }
  }

  async function handlePush() {
    setPushing(true);
    try {
      const result = await pushKnowledge();
      toast.success(result.message);
    } catch {
      toast.error('Failed to push knowledge to remote');
    } finally {
      setPushing(false);
    }
  }

  return (
    <div data-testid="knowledge-page">
      <AppHeader title="Knowledge">
        <Button
          variant="outline"
          size="sm"
          onClick={handlePush}
          disabled={pushing}
          data-testid="push-knowledge-btn"
        >
          <Upload className="mr-1.5 h-4 w-4" />
          {pushing ? 'Pushing…' : 'Push to remote'}
        </Button>
      </AppHeader>
      <div className="space-y-4 p-4 sm:p-6">
        <KnowledgeFilter onFilter={setFilter} />

        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-2">
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
          </div>
        ) : error ? (
          <p className="py-8 text-center text-destructive">{error}</p>
        ) : items.length === 0 ? (
          <p className="py-8 text-center text-muted-foreground">
            No knowledge items yet. Knowledge is extracted from completed sessions.
          </p>
        ) : (
          <KnowledgeList
            items={items}
            onDelete={handleDelete}
            onRefresh={() => fetchKnowledge(filter)}
          />
        )}
      </div>
    </div>
  );
}
