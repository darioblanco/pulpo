import { useState, useEffect, useCallback, useRef } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { CultureList } from '@/components/culture/culture-list';
import { CultureFilter } from '@/components/culture/culture-filter';
import { CultureFileBrowser } from '@/components/culture/culture-file-browser';
import { Skeleton } from '@/components/ui/skeleton';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import {
  listCulture,
  deleteCulture,
  pushCulture,
  approveCulture,
  getCultureSyncStatus,
} from '@/api/client';
import type { Culture, SyncStatus } from '@/api/types';
import { useSSE } from '@/hooks/use-sse';
import { toast } from 'sonner';
import { Upload, RefreshCw } from 'lucide-react';

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
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const { cultureVersion } = useSSE();
  const prevVersionRef = useRef(cultureVersion);

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

  useEffect(() => {
    getCultureSyncStatus()
      .then(setSyncStatus)
      .catch(() => {});
  }, [cultureVersion]);

  // Auto-refresh when culture changes arrive via SSE
  useEffect(() => {
    if (cultureVersion !== prevVersionRef.current) {
      prevVersionRef.current = cultureVersion;
      fetchCulture(filter);
      toast.info('Culture updated from sync');
    }
  }, [cultureVersion, filter, fetchCulture]);

  async function handleDelete(id: string) {
    try {
      await deleteCulture(id);
      setItems((prev) => prev.filter((k) => k.id !== id));
      toast.success('Culture item deleted');
    } catch {
      toast.error('Failed to delete culture item');
    }
  }

  async function handleApprove(id: string) {
    try {
      const result = await approveCulture(id);
      setItems((prev) => prev.map((k) => (k.id === id ? result.culture : k)));
      toast.success('Culture item approved');
    } catch {
      toast.error('Failed to approve culture item');
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
        {syncStatus?.enabled && (
          <Badge
            variant={syncStatus.last_error ? 'destructive' : 'secondary'}
            data-testid="sync-status-badge"
          >
            <RefreshCw className="mr-1 h-3 w-3" />
            {syncStatus.last_error
              ? 'Sync error'
              : syncStatus.last_sync
                ? `Synced (${syncStatus.total_syncs})`
                : 'Sync enabled'}
          </Badge>
        )}
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
        <Tabs defaultValue="files" data-testid="culture-tabs">
          <TabsList>
            <TabsTrigger value="files" data-testid="files-tab">
              Files
            </TabsTrigger>
            <TabsTrigger value="entries" data-testid="entries-tab">
              Entries
            </TabsTrigger>
          </TabsList>
          <TabsContent value="files">
            <CultureFileBrowser />
          </TabsContent>
          <TabsContent value="entries">
            <CultureFilter onFilter={setFilter} />

            {loading ? (
              <div data-testid="loading-skeleton" className="mt-4 space-y-2">
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
                onApprove={handleApprove}
                onRefresh={() => fetchCulture(filter)}
              />
            )}
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
