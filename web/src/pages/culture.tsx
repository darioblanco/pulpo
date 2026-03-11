import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { CultureList } from '@/components/culture/culture-list';
import { CultureFilter } from '@/components/culture/culture-filter';
import { Skeleton } from '@/components/ui/skeleton';
import { Button } from '@/components/ui/button';
import { listCulture, deleteCulture, pushCulture } from '@/api/client';
import type { Culture } from '@/api/types';
import { toast } from 'sonner';
import { Upload } from 'lucide-react';

export interface CultureFilterParams {
  kind?: string;
  repo?: string;
  ink?: string;
}

export function CulturePage() {
  const [items, setItems] = useState<Culture[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<CultureFilterParams>({});
  const [pushing, setPushing] = useState(false);

  const fetchCulture = useCallback(async (params: CultureFilterParams) => {
    setLoading(true);
    try {
      const data = await listCulture({ ...params, limit: 100 });
      setItems(data.culture);
      setError(null);
    } catch {
      setError('Failed to load culture');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchCulture(filter);
  }, [filter, fetchCulture]);

  async function handleDelete(id: string) {
    try {
      await deleteCulture(id);
      setItems((prev) => prev.filter((k) => k.id !== id));
      toast.success('Culture item deleted');
    } catch {
      toast.error('Failed to delete culture item');
    }
  }

  async function handlePush() {
    setPushing(true);
    try {
      const result = await pushCulture();
      toast.success(result.message);
    } catch {
      toast.error('Failed to push culture to remote');
    } finally {
      setPushing(false);
    }
  }

  return (
    <div data-testid="culture-page">
      <AppHeader title="Culture">
        <Button
          variant="outline"
          size="sm"
          onClick={handlePush}
          disabled={pushing}
          data-testid="push-culture-btn"
        >
          <Upload className="mr-1.5 h-4 w-4" />
          {pushing ? 'Pushing…' : 'Push to remote'}
        </Button>
      </AppHeader>
      <div className="space-y-4 p-4 sm:p-6">
        <CultureFilter onFilter={setFilter} />

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
            No culture items yet. Culture is extracted from completed sessions.
          </p>
        ) : (
          <CultureList
            items={items}
            onDelete={handleDelete}
            onRefresh={() => fetchCulture(filter)}
          />
        )}
      </div>
    </div>
  );
}
