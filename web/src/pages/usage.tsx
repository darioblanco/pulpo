import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { getUsageSessions } from '@/api/client';
import { toast } from 'sonner';
import { Wallet } from 'lucide-react';
import type { UsageSessionsResponse, DimensionRollup } from '@/api/types';

/** Compact a token count: 1234 → "1.2K", 4_500_000 → "4.5M". */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** Every cost comes from a structured usage reader (no output-scraping fallback), so
 * this is always exact. */
function fmtCost(c: number | null): string {
  if (c == null) return '—';
  return `$${c.toFixed(2)}`;
}

/** A cost-attribution breakdown by repo (most expensive first). */
function DimensionTable({ title, rows }: { title: string; rows: DimensionRollup[] }) {
  if (rows.length === 0) return null;
  return (
    <div className="rounded-lg border border-border" data-testid={`dimension-${title}`}>
      <div className="border-b border-border bg-muted/50 px-4 py-2 text-xs font-medium text-muted-foreground">
        {title}
      </div>
      <table className="w-full text-sm">
        <tbody>
          {rows.map((r) => (
            <tr key={r.label} className="border-b border-border last:border-0">
              <td className="px-4 py-2 font-medium">{r.label}</td>
              <td className="px-4 py-2 text-right text-xs text-muted-foreground">
                {r.session_count} session{r.session_count === 1 ? '' : 's'}
              </td>
              <td className="px-4 py-2 text-right font-mono text-xs text-muted-foreground">
                {fmtTokens(r.total_tokens)}
              </td>
              <td className="px-4 py-2 text-right font-mono text-xs font-medium">
                {fmtCost(r.total_cost_usd)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function UsagePage() {
  const [data, setData] = useState<UsageSessionsResponse | null>(null);
  const [loading, setLoading] = useState(true);

  const fetchUsage = useCallback(async () => {
    try {
      setData(await getUsageSessions());
    } catch {
      toast.error('Failed to load usage');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchUsage();
  }, [fetchUsage]);

  const totalCost = data?.sessions.reduce<number | null>((sum, s) => {
    if (s.cost_usd == null) return sum;
    return (sum ?? 0) + s.cost_usd;
  }, null);
  const totalTokens = data?.sessions.reduce((sum, s) => sum + s.total_tokens, 0) ?? 0;

  return (
    <div data-testid="usage-page">
      <AppHeader title="Usage" />
      <div className="space-y-6 p-4 sm:p-6">
        {loading ? (
          <Skeleton className="h-40 w-full" data-testid="usage-loading" />
        ) : !data || data.sessions.length === 0 ? (
          <div className="py-12 text-center" data-testid="usage-empty">
            <Wallet className="mx-auto mb-3 h-10 w-10 text-muted-foreground" />
            <p className="text-muted-foreground">No usage data yet</p>
            <p className="mt-1 text-sm text-muted-foreground">
              Spend appears once an agent session reports tokens.
            </p>
          </div>
        ) : (
          <>
            <Card data-testid="usage-total-card">
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium text-muted-foreground">
                  Total spend on {data.node_name}
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-semibold">{fmtCost(totalCost ?? null)}</div>
                <div className="text-xs text-muted-foreground">
                  {fmtTokens(totalTokens)} tokens · {data.sessions.length} session
                  {data.sessions.length === 1 ? '' : 's'}
                </div>
              </CardContent>
            </Card>

            {data.repos.length > 0 && (
              <div className="grid gap-3 sm:grid-cols-2">
                <DimensionTable title="By repo" rows={data.repos} />
              </div>
            )}

            <div className="overflow-x-auto rounded-lg border border-border">
              <table className="w-full text-sm" data-testid="usage-table">
                <thead>
                  <tr className="border-b border-border bg-muted/50 text-left text-xs text-muted-foreground">
                    <th className="px-4 py-2.5 font-medium">Session</th>
                    <th className="px-4 py-2.5 font-medium">Source</th>
                    <th className="px-4 py-2.5 text-right font-medium">Tokens</th>
                    <th className="px-4 py-2.5 text-right font-medium">Cost</th>
                  </tr>
                </thead>
                <tbody>
                  {data.sessions.map((s) => (
                    <tr
                      key={s.session_id}
                      data-testid={`usage-row-${s.session_name}`}
                      className="border-b border-border last:border-0"
                    >
                      <td className="px-4 py-3 font-medium">{s.session_name}</td>
                      <td className="px-4 py-3 text-xs text-muted-foreground">
                        {s.usage_source?.replace(/-jsonl$/, '') ?? '—'}
                      </td>
                      <td className="px-4 py-3 text-right font-mono text-xs">
                        {fmtTokens(s.total_tokens)}
                      </td>
                      <td className="px-4 py-3 text-right font-mono text-xs">
                        {fmtCost(s.cost_usd)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
